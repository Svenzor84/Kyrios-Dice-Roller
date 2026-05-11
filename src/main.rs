#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kyrios::game::{Game, GameLogic, GameState, MapStyle};
use kyrios::components::{Collision2d, CollisionType, Sprite};
use kyrios::types::{Point2d, Point3d, Transform};
use kyrios::widget::{Widget, WidgetStyles, WidgetStates};
use kyrios::registry::Entity;
use kyrios::renderer::Color;
use rand::Rng;
use slotmap::DefaultKey;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

// ── Screen ────────────────────────────────────────────────────────────────────
const SCREEN_W: u32 = 960;
const SCREEN_H: u32 = 640;

// ── Physics ───────────────────────────────────────────────────────────────────
const DIE_SIZE: f32 = 96.0;
const MAX_DICE: u32 = 8;

#[derive(Clone, Copy, PartialEq)]
enum TrayMaterial { Felt, Hardwood, Carpet, Ice }

impl TrayMaterial {
    fn next(self) -> Self {
        match self {
            Self::Felt     => Self::Hardwood,
            Self::Hardwood => Self::Carpet,
            Self::Carpet   => Self::Ice,
            Self::Ice      => Self::Felt,
        }
    }
    fn label(self) -> &'static str {
        match self { Self::Felt => "FELT", Self::Hardwood => "HARDWOOD", Self::Carpet => "CARPET", Self::Ice => "ICE" }
    }
    fn tex_id(self) -> &'static str {
        match self { Self::Felt => "tray_felt", Self::Hardwood => "tray_hardwood", Self::Carpet => "tray_carpet", Self::Ice => "tray_ice" }
    }
    // Fraction of velocity remaining after one full second (applied as powf(delta))
    fn damping(self) -> f32 {
        match self { Self::Felt => 0.10, Self::Hardwood => 0.30, Self::Carpet => 0.02, Self::Ice => 0.55 }
    }
    // Speed (px/s) below which a die is considered settled
    fn settle_threshold(self) -> f32 {
        match self { Self::Felt => 24.0, Self::Hardwood => 18.0, Self::Carpet => 70.0, Self::Ice => 16.0 }
    }
}

// ── Left panel: die-select buttons (sprite entities, click via hit-test) ─────
const BTN_SIZE: f32 = 70.0;
const BTN_X: u32 = 10;
const BTN_GAP: u32 = 8;
// 7 × 70 + 6 × 8 = 538px; top margin = (640 − 538) / 2 = 51px
const BTN_START_Y: u32 = 51;

// ── Centre: dice-rolling tray ─────────────────────────────────────────────────
const TRAY_X: f32 = 92.0;
const TRAY_Y: f32 = 20.0;
const TRAY_W: f32 = 660.0;
const TRAY_H: f32 = 600.0;
const RAIL_W: f32 = 10.0; // visible tray wall thickness

// ── Right panel: results (widgets) ───────────────────────────────────────────
const PANEL_X: u32 = 768;
const PANEL_W: u32 = 180;
const RESULT_LINE_H: u32 = 26;

// ── Die table ─────────────────────────────────────────────────────────────────
const BTNS: [(&str, u32, &str); 7] = [
    ("d4",  4,   "d4"),
    ("d6",  6,   "d6"),
    ("d8",  8,   "d8"),
    ("d10", 10,  "d10"),
    ("d12", 12,  "d12"),
    ("d20", 20,  "d20"),
    ("d%",  100, "d%"),
];

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum AppState { Splash, Playing }

#[derive(Clone, Copy, PartialEq)]
enum DieType { Normal, PercentileTens, PercentileOnes }

struct RollingDie {
    entity: DefaultKey,
    sides: u32,
    die_type: DieType,
    x: f32, y: f32,
    vx: f32, vy: f32,
    result: Option<u32>,
}

struct DiceState {
    app_state: AppState,

    // Tray material (physics preset)
    material: TrayMaterial,
    material_key: Option<DefaultKey>,

    // Drag / hold
    dragging: bool,
    drag_idx: usize,    // index into BTNS
    drag_count: u32,
    // Cursor velocity tracking — EMA-smoothed, used as launch velocity on release
    drag_vx: f32, drag_vy: f32,
    prev_drag_x: i32, prev_drag_y: i32,

    // Active rolling dice
    rolling_dice: Vec<RollingDie>,
    results_shown: bool,

    // Permanent entities
    tray_entity:    Option<DefaultKey>,
    wall_keys:      [Option<DefaultKey>; 4],  // [left, right, top, bottom] tray rails
    btn_entities:   [Option<DefaultKey>; 7],
    cursor_entity:  Option<DefaultKey>,  // die sprite that follows the cursor

    // Widgets
    count_label_key:   Option<DefaultKey>,       // "2d6" badge near cursor
    result_title_key:  Option<DefaultKey>,       // "3d6"
    result_line_keys:  [Option<DefaultKey>; 8],  // one row per die result
    result_sep_key:    Option<DefaultKey>,        // horizontal separator bar
    result_total_key:  Option<DefaultKey>,        // "= 11"

    // Splash
    splash_bg_key:    Option<DefaultKey>,
    splash_title_key: Option<DefaultKey>,
    splash_sub_key:   Option<DefaultKey>,
    splash_inst_key:  Option<DefaultKey>,
}

impl DiceState {
    fn new() -> Self {
        Self {
            app_state: AppState::Splash,
            material: TrayMaterial::Felt, material_key: None,
            dragging: false, drag_idx: 0, drag_count: 1,
            drag_vx: 0.0, drag_vy: 0.0, prev_drag_x: 0, prev_drag_y: 0,
            rolling_dice: Vec::new(), results_shown: false,
            tray_entity: None, wall_keys: [None; 4], btn_entities: [None; 7], cursor_entity: None,
            count_label_key: None,
            result_title_key: None, result_line_keys: [None; 8],
            result_sep_key: None, result_total_key: None,
            splash_bg_key: None, splash_title_key: None,
            splash_sub_key: None, splash_inst_key: None,
        }
    }
}

// ── Styles ────────────────────────────────────────────────────────────────────

fn hidden_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 0, g: 0, b: 0, a: 0 },
        hover_color: None,
        font_color: Color { r: 0, g: 0, b: 0, a: 0 },
        font_hover_color: None,
    }
}

fn result_title_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 20, g: 20, b: 50, a: 230 },
        hover_color: None,
        font_color: Color { r: 255, g: 210, b: 80, a: 255 },
        font_hover_color: None,
    }
}


// Cyan=exact max, Green=top third, Yellow=middle, Orange=bottom third, Red=exact min
fn result_color(result: u32, max: u32) -> Color {
    if result == max { return Color { r: 0,   g: 220, b: 220, a: 255 }; } // cyan
    if result == 1   { return Color { r: 220, g: 30,  b: 30,  a: 255 }; } // red
    match (result * 100) / max {
        67..=99 => Color { r: 50,  g: 200, b: 50,  a: 255 }, // green
        34..=66 => Color { r: 225, g: 210, b: 30,  a: 255 }, // yellow
        _       => Color { r: 255, g: 140, b: 0,   a: 255 }, // orange
    }
}

fn result_line_styles(font_color: Color) -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 20, g: 20, b: 45, a: 180 },
        hover_color: None,
        font_color,
        font_hover_color: None,
    }
}

fn result_sep_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 150, g: 150, b: 180, a: 160 },
        hover_color: None,
        font_color: Color { r: 0, g: 0, b: 0, a: 0 },
        font_hover_color: None,
    }
}

fn count_label_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 40, g: 40, b: 40, a: 220 },
        hover_color: None,
        font_color: Color { r: 255, g: 255, b: 255, a: 255 },
        font_hover_color: None,
    }
}

fn splash_bg_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 10, g: 10, b: 30, a: 210 },
        hover_color: None,
        font_color: Color { r: 0, g: 0, b: 0, a: 0 },
        font_hover_color: None,
    }
}

fn splash_title_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 160, g: 40, b: 40, a: 255 },
        hover_color: None,
        font_color: Color { r: 255, g: 220, b: 80, a: 255 },
        font_hover_color: None,
    }
}

fn splash_sub_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 100, g: 25, b: 25, a: 255 },
        hover_color: None,
        font_color: Color { r: 255, g: 170, b: 60, a: 255 },
        font_hover_color: None,
    }
}

fn material_toggle_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 40, g: 30, b: 15, a: 200 },
        hover_color: Some(Color { r: 70, g: 55, b: 25, a: 230 }),
        font_color: Color { r: 210, g: 165, b: 75, a: 255 },
        font_hover_color: Some(Color { r: 255, g: 220, b: 120, a: 255 }),
    }
}

fn splash_inst_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 25, g: 25, b: 70, a: 220 },
        hover_color: None,
        font_color: Color { r: 180, g: 200, b: 255, a: 255 },
        font_hover_color: None,
    }
}

// ── Roll logic ────────────────────────────────────────────────────────────────

fn do_roll(die_type: DieType, sides: u32) -> u32 {
    let mut rng = rand::thread_rng();
    match die_type {
        DieType::Normal         => rng.gen_range(1..=sides),
        DieType::PercentileTens => rng.gen_range(0..=9u32) * 10,
        DieType::PercentileOnes => rng.gen_range(0..=9u32),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

// All assets are embedded into the binary at compile time and extracted to a
// per-process temp directory on startup. This makes the exe fully self-contained.
struct TempAssets {
    dir: PathBuf,
}

impl TempAssets {
    fn extract() -> Self {
        let dir = std::env::temp_dir()
            .join(format!("kyrios-dice-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets/sprites")).unwrap();
        std::fs::create_dir_all(dir.join("assets/fonts")).unwrap();

        macro_rules! embed {
            ($subpath:literal) => {
                std::fs::write(
                    dir.join(concat!("assets/", $subpath)),
                    include_bytes!(concat!("../assets/", $subpath)),
                ).unwrap();
            };
        }

        embed!("fonts/Roboto-Regular.ttf");
        embed!("sprites/tray_felt.png");
        embed!("sprites/tray_hardwood.png");
        embed!("sprites/tray_carpet.png");
        embed!("sprites/tray_ice.png");
        embed!("sprites/tray_bg.png");   // placeholder rail texture
        embed!("sprites/d4.png");
        embed!("sprites/d6.png");
        embed!("sprites/d8.png");
        embed!("sprites/d10.png");
        embed!("sprites/d12.png");
        embed!("sprites/d20.png");
        embed!("sprites/d_percent.png");

        TempAssets { dir }
    }

    fn path(&self, relative: &str) -> String {
        self.dir.join(relative).to_string_lossy().into_owned()
    }
}

impl Drop for TempAssets {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() {
    // Install a panic hook that writes to a log file next to the exe.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC] {info}\n");
        eprintln!("{msg}");
        let log = exe_dir().join("dice_roller.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            use std::io::Write;
            let _ = write!(f, "{msg}");
        }
    }));

    let assets = TempAssets::extract();

    let shared: Rc<RefCell<DiceState>> = Rc::new(RefCell::new(DiceState::new()));

    let mut game = Game::new(
        "Extreme Dice Roller: Epic Edition",
        MapStyle::Default,
        PathBuf::from(assets.path("assets")),
    );

    game.texture_list.insert("tray_felt".to_string(),     assets.path("assets/sprites/tray_felt.png"));
    game.texture_list.insert("tray_hardwood".to_string(), assets.path("assets/sprites/tray_hardwood.png"));
    game.texture_list.insert("tray_carpet".to_string(),   assets.path("assets/sprites/tray_carpet.png"));
    game.texture_list.insert("tray_ice".to_string(),      assets.path("assets/sprites/tray_ice.png"));
    game.texture_list.insert("rail".to_string(),           assets.path("assets/sprites/tray_bg.png"));
    for (_, _, tex_id) in &BTNS {
        let path = if *tex_id == "d%" {
            assets.path("assets/sprites/d_percent.png")
        } else {
            assets.path(&format!("assets/sprites/{}.png", tex_id))
        };
        game.texture_list.insert(tex_id.to_string(), path);
    }
    game.font_list.insert("default".to_string(), assets.path("assets/fonts/Roboto-Regular.ttf"));

    let ss = shared.clone();
    game.startup_scripts.push(GameLogic {
        target: None,
        script: Box::new(move |_, state| { setup_ui(state, &mut ss.borrow_mut()); }),
    });

    let ss2 = shared.clone();
    game.game_scripts.push(GameLogic {
        target: None,
        script: Box::new(move |_, state| { update(state, &mut ss2.borrow_mut()); }),
    });

    // game_systems run after collision_2d_system — used for collision response
    let ss3 = shared.clone();
    game.game_systems.push(Box::new(move |state| {
        handle_collisions(state, &mut ss3.borrow_mut());
    }));

    game.init();
    // Cap at ~240fps to prevent SDL TTF texture ops from dominating at uncapped rates.
    const TARGET_FRAME_US: u64 = 4167;
    loop {
        let t = std::time::Instant::now();
        if game.update() == -1 { break; }
        let elapsed = t.elapsed().as_micros() as u64;
        if elapsed < TARGET_FRAME_US {
            std::thread::sleep(std::time::Duration::from_micros(TARGET_FRAME_US - elapsed));
        }
    }
}

// ── Setup ─────────────────────────────────────────────────────────────────────

fn setup_ui(state: &mut GameState, ds: &mut DiceState) {
    // Tray — first entity so it always has the lowest slot index and renders behind all others
    let mut tray = Entity::new();
    tray.add_component(Sprite {
        name: "tray".to_string(), texture_id: ds.material.tex_id().to_string(),
        width: 4, height: 4, x: 0, y: 0, animations: None, animation_index: 0,
    });
    let mut tf = Transform::new();
    tf.set_length(TRAY_W); tf.set_width(TRAY_H);
    tf.set_x(TRAY_X); tf.set_y(TRAY_Y);
    tray.add_component(tf);
    ds.tray_entity = Some(state.entities.entities.insert(tray));

    // Tray rail entities — visible walls, also carry Collision2d for the collision system.
    // Layout: [0]=left, [1]=right, [2]=top, [3]=bottom.
    // Top/bottom rails span the full tray width + corners; left/right cover just the side height.
    let rails: [(f32, f32, f32, f32); 4] = [
        (TRAY_X - RAIL_W,               TRAY_Y,               RAIL_W,                  TRAY_H),
        (TRAY_X + TRAY_W,               TRAY_Y,               RAIL_W,                  TRAY_H),
        (TRAY_X - RAIL_W,               TRAY_Y - RAIL_W,      TRAY_W + 2.0 * RAIL_W,  RAIL_W),
        (TRAY_X - RAIL_W,               TRAY_Y + TRAY_H,      TRAY_W + 2.0 * RAIL_W,  RAIL_W),
    ];
    for (i, (rx, ry, rw, rh)) in rails.iter().enumerate() {
        let mut rail = Entity::new();
        rail.add_component(Sprite {
            name: format!("rail_{}", i), texture_id: "rail".to_string(),
            width: 4, height: 4, x: 0, y: 0, animations: None, animation_index: 0,
        });
        let mut tf = Transform::new();
        tf.set_length(*rw); tf.set_width(*rh);
        tf.set_x(*rx); tf.set_y(*ry);
        rail.add_component(tf);
        // length = x extent, width = y extent, height = 1 for 2D
        rail.add_component(Collision2d::new(
            CollisionType::Square,
            *rw, *rh, 1.0, 0.0,
            Point3d { x: *rx, y: *ry, z: 0.0 },
            Point2d { x: 0.0, y: 0.0 },
        ));
        ds.wall_keys[i] = Some(state.entities.entities.insert(rail));
    }

    // Die-select button entities (left panel, slots 1–7)
    for (i, (_, _, tex_id)) in BTNS.iter().enumerate() {
        let mut btn = Entity::new();
        btn.add_component(Sprite {
            name: format!("btn_{}", tex_id), texture_id: tex_id.to_string(),
            width: DIE_SIZE as u32, height: DIE_SIZE as u32,
            x: 0, y: 0, animations: None, animation_index: 0,
        });
        let mut tf = Transform::new();
        tf.set_length(BTN_SIZE); tf.set_width(BTN_SIZE);
        tf.set_x(BTN_X as f32);
        tf.set_y((BTN_START_Y + i as u32 * (BTN_SIZE as u32 + BTN_GAP)) as f32);
        btn.add_component(tf);
        ds.btn_entities[i] = Some(state.entities.entities.insert(btn));
    }

    // Cursor entity (slot 8) — die sprite that follows the mouse while dragging.
    // Created here so it always has a lower slot than rolling-die entities (which
    // are created/destroyed dynamically on each roll).
    let mut cursor = Entity::new();
    cursor.add_component(Sprite {
        name: "cursor".to_string(), texture_id: "d6".to_string(),
        width: DIE_SIZE as u32, height: DIE_SIZE as u32,
        x: 0, y: 0, animations: None, animation_index: 0,
    });
    let mut tf = Transform::new();
    tf.set_length(BTN_SIZE); tf.set_width(BTN_SIZE);
    tf.set_x(-2000.0); tf.set_y(-2000.0);
    cursor.add_component(tf);
    ds.cursor_entity = Some(state.entities.entities.insert(cursor));

    // Root menu widget — fullscreen transparent container for Kyrios ui system
    let root = Widget {
        label: String::new(),
        position: Point2d { x: 0u32, y: 0u32 },
        width: SCREEN_W, height: SCREEN_H,
        visible: true, active: true,
        styles: hidden_styles(),
        ..Default::default()
    };
    let root_key = state.widgets.insert(root);
    state.widgets[root_key].key = Some(root_key);
    state.menus.push(root_key);

    // Right panel: material cycle toggle (always visible, top of panel)
    let mt = Widget {
        label: ds.material.label().to_string(),
        position: Point2d { x: PANEL_X, y: 20u32 },
        width: PANEL_W, height: 35,
        visible: true, active: true,
        styles: material_toggle_styles(),
        ..Default::default()
    };
    let mtk = state.widgets.insert(mt);
    state.widgets[root_key].children.push(mtk);
    ds.material_key = Some(mtk);

    // Right panel: result title — position set dynamically by show_results
    let rt = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: 0u32 },
        width: PANEL_W, height: 28,
        visible: true, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    let rtk = state.widgets.insert(rt);
    state.widgets[root_key].children.push(rtk);
    ds.result_title_key = Some(rtk);

    // Right panel: one result line per possible die (up to 8) — positions set dynamically
    for i in 0..8usize {
        let line = Widget {
            label: String::new(),
            position: Point2d { x: PANEL_X, y: 0u32 },
            width: PANEL_W, height: RESULT_LINE_H,
            visible: true, active: false,
            styles: hidden_styles(),
            ..Default::default()
        };
        let lk = state.widgets.insert(line);
        state.widgets[root_key].children.push(lk);
        ds.result_line_keys[i] = Some(lk);
    }

    // Right panel: separator bar — position set dynamically
    let sep = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: 0u32 },
        width: PANEL_W, height: 4,
        visible: true, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    let sepk = state.widgets.insert(sep);
    state.widgets[root_key].children.push(sepk);
    ds.result_sep_key = Some(sepk);

    // Right panel: result total
    let rto = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: 0u32 },
        width: PANEL_W, height: 30,
        visible: true, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    let rtok = state.widgets.insert(rto);
    state.widgets[root_key].children.push(rtok);
    ds.result_total_key = Some(rtok);

    // Splash overlay
    let sbg = Widget {
        label: String::new(),
        position: Point2d { x: 0u32, y: 0u32 },
        width: SCREEN_W, height: SCREEN_H,
        visible: true, active: false,
        styles: splash_bg_styles(),
        ..Default::default()
    };
    ds.splash_bg_key = Some(state.widgets.insert(sbg));
    state.widgets[root_key].children.push(ds.splash_bg_key.unwrap());

    let st = Widget {
        label: "EXTREME DICE ROLLER".to_string(),
        position: Point2d { x: 330u32, y: 230u32 },
        width: 300, height: 55,
        visible: true, active: false,
        styles: splash_title_styles(),
        ..Default::default()
    };
    ds.splash_title_key = Some(state.widgets.insert(st));
    state.widgets[root_key].children.push(ds.splash_title_key.unwrap());

    let ss = Widget {
        label: "EPIC EDITION".to_string(),
        position: Point2d { x: 400u32, y: 295u32 },
        width: 160, height: 38,
        visible: true, active: false,
        styles: splash_sub_styles(),
        ..Default::default()
    };
    ds.splash_sub_key = Some(state.widgets.insert(ss));
    state.widgets[root_key].children.push(ds.splash_sub_key.unwrap());

    let si = Widget {
        label: "click or press any key".to_string(),
        position: Point2d { x: 368u32, y: 390u32 },
        width: 224, height: 35,
        visible: true, active: false,
        styles: splash_inst_styles(),
        ..Default::default()
    };
    ds.splash_inst_key = Some(state.widgets.insert(si));
    state.widgets[root_key].children.push(ds.splash_inst_key.unwrap());

    // Count label — added last so it renders on top of all other widgets.
    // Shows "2d6", "3d8", etc. near the cursor while dragging.
    let cl = Widget {
        label: String::new(),
        position: Point2d { x: 0u32, y: 0u32 },
        width: 50, height: 25,
        visible: true, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    ds.count_label_key = Some(state.widgets.insert(cl));
    state.widgets[root_key].children.push(ds.count_label_key.unwrap());
}

// ── Per-frame dispatch ────────────────────────────────────────────────────────

fn update(state: &mut GameState, ds: &mut DiceState) {
    match ds.app_state {
        AppState::Splash  => update_splash(state, ds),
        AppState::Playing => update_playing(state, ds),
    }
}

// ── Splash ────────────────────────────────────────────────────────────────────

fn update_splash(state: &mut GameState, ds: &mut DiceState) {
    let advance = !state.events.key_down.is_empty()
        || state.events.mouse.left_down
        || state.events.mouse.right_down;
    if !advance { return; }

    for key in [ds.splash_bg_key, ds.splash_title_key, ds.splash_sub_key, ds.splash_inst_key] {
        if let Some(k) = key {
            if let Some(w) = state.widgets.get_mut(k) {
                w.label  = String::new();
                w.styles = hidden_styles();
            }
        }
    }
    ds.app_state = AppState::Playing;
}

// ── Playing ───────────────────────────────────────────────────────────────────

fn update_playing(state: &mut GameState, ds: &mut DiceState) {
    // Material toggle click — cycle preset and update label
    if let Some(k) = ds.material_key {
        if let Some(w) = state.widgets.get_mut(k) {
            if matches!(w.state, WidgetStates::MouseLeftUp) && !ds.dragging {
                ds.material = ds.material.next();
                w.label = ds.material.label().to_string();
                // Swap the tray background texture
                if let Some(te) = ds.tray_entity {
                    if let Some(entity) = state.entities.entities.get_mut(te) {
                        if let Some(sprite) = entity.get_component_mut::<Sprite>() {
                            sprite.texture_id = ds.material.tex_id().to_string();
                        }
                    }
                }
            }
        }
    }

    handle_drag_input(state, ds);
    if !ds.rolling_dice.is_empty() {
        update_rolling(state, ds);
    }
}

fn handle_drag_input(state: &mut GameState, ds: &mut DiceState) {
    let mx = state.events.mouse.position.x;
    let my = state.events.mouse.position.y;

    // Start a drag — checked independently so a same-frame left_down + left_up
    // (fast click) can still begin AND end the drag in one pass.
    if !ds.dragging && state.events.mouse.left_down {
        for i in 0..BTNS.len() {
            let bx = BTN_X;
            let by = BTN_START_Y + i as u32 * (BTN_SIZE as u32 + BTN_GAP);
            let bs = BTN_SIZE as u32;
            if mx >= bx && mx < bx + bs && my >= by && my < by + bs {
                begin_drag(state, ds, i);
                break;
            }
        }
    }

    // While dragging: track cursor velocity, accumulate dice, release
    if ds.dragging {
        // EMA-smooth the cursor velocity so a single jittery frame doesn't spike it
        let delta_secs = (state.tick.delta as f32 / 1_000_000.0).max(0.0001);
        let raw_vx = (mx as i32 - ds.prev_drag_x) as f32 / delta_secs;
        let raw_vy = (my as i32 - ds.prev_drag_y) as f32 / delta_secs;
        const SMOOTH: f32 = 0.35;
        ds.drag_vx = ds.drag_vx * (1.0 - SMOOTH) + raw_vx * SMOOTH;
        ds.drag_vy = ds.drag_vy * (1.0 - SMOOTH) + raw_vy * SMOOTH;
        ds.prev_drag_x = mx as i32;
        ds.prev_drag_y = my as i32;

        set_cursor_pos(state, ds, mx, my);
        refresh_count_label(state, ds, mx, my);

        if state.events.mouse.right_down {
            let is_percentile = BTNS[ds.drag_idx].1 == 100;
            if !is_percentile && ds.drag_count < MAX_DICE {
                ds.drag_count += 1;
                refresh_count_label(state, ds, mx, my);
            }
        }

        if state.events.mouse.left_up {
            end_drag(state, ds);
        }
    }
}

fn begin_drag(state: &mut GameState, ds: &mut DiceState, btn_idx: usize) {
    // Clear any in-progress or completed roll
    for rd in ds.rolling_dice.drain(..) {
        state.entities.entities.remove(rd.entity);
    }
    hide_results(state, ds);
    ds.results_shown = false;

    ds.dragging   = true;
    ds.drag_idx   = btn_idx;
    ds.drag_count = 1;
    ds.drag_vx = 0.0;
    ds.drag_vy = 0.0;
    ds.prev_drag_x = state.events.mouse.position.x as i32;
    ds.prev_drag_y = state.events.mouse.position.y as i32;

    // Update cursor sprite to match selected die type
    if let Some(ce) = ds.cursor_entity {
        if let Some(entity) = state.entities.entities.get_mut(ce) {
            if let Some(sprite) = entity.get_component_mut::<Sprite>() {
                sprite.texture_id = BTNS[btn_idx].2.to_string();
            }
        }
    }

    let mx = state.events.mouse.position.x;
    let my = state.events.mouse.position.y;
    set_cursor_pos(state, ds, mx, my);
    refresh_count_label(state, ds, mx, my);
}

fn end_drag(state: &mut GameState, ds: &mut DiceState) {
    ds.dragging = false;

    // Hide cursor entity
    if let Some(ce) = ds.cursor_entity {
        if let Some(entity) = state.entities.entities.get_mut(ce) {
            if let Some(tf) = entity.get_component_mut::<Transform>() {
                tf.set_x(-2000.0);
                tf.set_y(-2000.0);
            }
        }
    }

    // Hide count label
    if let Some(clk) = ds.count_label_key {
        if let Some(w) = state.widgets.get_mut(clk) {
            w.label  = String::new();
            w.styles = hidden_styles();
        }
    }

    let count  = ds.drag_count;
    let sides  = BTNS[ds.drag_idx].1;
    let tex_id = BTNS[ds.drag_idx].2;
    let vx = ds.drag_vx;
    let vy = ds.drag_vy;
    // Offset so the die centre lands under the cursor tip, not the top-left corner
    let cursor_x = state.events.mouse.position.x as f32 - DIE_SIZE / 2.0;
    let cursor_y = state.events.mouse.position.y as f32 - DIE_SIZE / 2.0;
    spawn_dice(state, ds, count, sides, tex_id, cursor_x, cursor_y, vx, vy);
}

fn set_cursor_pos(state: &mut GameState, ds: &DiceState, mx: u32, my: u32) {
    if let Some(ce) = ds.cursor_entity {
        if let Some(entity) = state.entities.entities.get_mut(ce) {
            if let Some(tf) = entity.get_component_mut::<Transform>() {
                tf.set_x(mx as f32 - BTN_SIZE / 2.0);
                tf.set_y(my as f32 - BTN_SIZE / 2.0);
            }
        }
    }
}

fn refresh_count_label(state: &mut GameState, ds: &DiceState, mx: u32, my: u32) {
    if let Some(clk) = ds.count_label_key {
        if let Some(w) = state.widgets.get_mut(clk) {
            let label_x = (mx + 38).min(SCREEN_W.saturating_sub(55));
            let label_y = my.saturating_sub(28);
            w.position = Point2d { x: label_x, y: label_y };
            let sides = BTNS[ds.drag_idx].1;
            let sides_str = if sides == 100 { "%".to_string() } else { sides.to_string() };
            w.label  = format!("{}d{}", ds.drag_count, sides_str);
            w.styles = count_label_styles();
        }
    }
}

// ── Rolling physics ───────────────────────────────────────────────────────────

fn spawn_dice(state: &mut GameState, ds: &mut DiceState, count: u32, sides: u32, tex_id: &'static str, cursor_x: f32, cursor_y: f32, base_vx: f32, base_vy: f32) {
    if sides == 100 {
        spawn_percentile_dice(state, ds, cursor_x, cursor_y, base_vx, base_vy);
        return;
    }

    let positions = cursor_spawn_positions(cursor_x, cursor_y, count);
    let mut rng = rand::thread_rng();
    let spread = (base_vx.abs().max(base_vy.abs()) * 0.2).max(40.0);

    for (px, py) in positions {
        let vx = base_vx + rng.gen_range(-spread..=spread);
        let vy = base_vy + rng.gen_range(-spread..=spread);

        let mut die = Entity::new();
        die.add_component(Sprite {
            name: "die".to_string(), texture_id: tex_id.to_string(),
            width: DIE_SIZE as u32, height: DIE_SIZE as u32,
            x: 0, y: 0, animations: None, animation_index: 0,
        });
        let mut tf = Transform::new();
        tf.set_length(DIE_SIZE); tf.set_width(DIE_SIZE);
        tf.set_x(px); tf.set_y(py);
        die.add_component(tf);
        die.add_component(Collision2d::square(DIE_SIZE, Point3d { x: px, y: py, z: 0.0 }));

        let key = state.entities.entities.insert(die);
        ds.rolling_dice.push(RollingDie { entity: key, sides, die_type: DieType::Normal, x: px, y: py, vx, vy, result: None });
    }
}

fn spawn_percentile_dice(state: &mut GameState, ds: &mut DiceState, cursor_x: f32, cursor_y: f32, base_vx: f32, base_vy: f32) {
    let positions = cursor_spawn_positions(cursor_x, cursor_y, 2);
    let mut rng = rand::thread_rng();
    let spread = (base_vx.abs().max(base_vy.abs()) * 0.2).max(40.0);

    // tens die uses the d% sprite, ones die uses the d10 sprite
    let dice_info = [("d%", DieType::PercentileTens), ("d10", DieType::PercentileOnes)];

    for ((px, py), (tex, die_type)) in positions.into_iter().zip(dice_info.into_iter()) {
        let vx = base_vx + rng.gen_range(-spread..=spread);
        let vy = base_vy + rng.gen_range(-spread..=spread);

        let mut die = Entity::new();
        die.add_component(Sprite {
            name: "die".to_string(), texture_id: tex.to_string(),
            width: DIE_SIZE as u32, height: DIE_SIZE as u32,
            x: 0, y: 0, animations: None, animation_index: 0,
        });
        let mut tf = Transform::new();
        tf.set_length(DIE_SIZE); tf.set_width(DIE_SIZE);
        tf.set_x(px); tf.set_y(py);
        die.add_component(tf);
        die.add_component(Collision2d::square(DIE_SIZE, Point3d { x: px, y: py, z: 0.0 }));

        let key = state.entities.entities.insert(die);
        ds.rolling_dice.push(RollingDie { entity: key, sides: 10, die_type, x: px, y: py, vx, vy, result: None });
    }
}

fn cursor_spawn_positions(cursor_x: f32, cursor_y: f32, count: u32) -> Vec<(f32, f32)> {
    let cx = cursor_x.clamp(TRAY_X, TRAY_X + TRAY_W - DIE_SIZE);
    let cy = cursor_y.clamp(TRAY_Y, TRAY_Y + TRAY_H - DIE_SIZE);
    if count == 1 {
        return vec![(cx, cy)];
    }
    let radius = (DIE_SIZE * 0.8).min(TRAY_W.min(TRAY_H) / 6.0);
    (0..count).map(|i| {
        let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
        let px = (cx + angle.cos() * radius).clamp(TRAY_X, TRAY_X + TRAY_W - DIE_SIZE);
        let py = (cy + angle.sin() * radius).clamp(TRAY_Y, TRAY_Y + TRAY_H - DIE_SIZE);
        (px, py)
    }).collect()
}

fn update_rolling(state: &mut GameState, ds: &mut DiceState) {
    let delta  = (state.tick.delta as f32 / 1_000_000.0).min(0.1);
    let damping = ds.material.damping().powf(delta);

    // Phase 1: position update + damping (wall bounce and die-die handled by collision system)
    for rd in &mut ds.rolling_dice {
        if rd.result.is_some() { continue; }
        rd.x += rd.vx * delta;
        rd.y += rd.vy * delta;
        rd.vx *= damping;
        rd.vy *= damping;
    }

    // Phase 2: sync positions to Transform so collision_2d_system sees current positions
    for rd in &mut ds.rolling_dice {
        if rd.result.is_some() { continue; }
        if let Some(entity) = state.entities.entities.get_mut(rd.entity) {
            if let Some(tf) = entity.get_component_mut::<Transform>() {
                tf.set_x(rd.x);
                tf.set_y(rd.y);
            }
        }
    }
    // collision_2d_system runs next (engine), then handle_collisions game_system applies responses
}

// Runs as a game_system — AFTER collision_2d_system has populated intersections.
// Applies wall bounce, die-die separation+swap, settle check, and result display.
fn handle_collisions(state: &mut GameState, ds: &mut DiceState) {
    if ds.rolling_dice.is_empty() { return; }

    let max_x = TRAY_X + TRAY_W - DIE_SIZE;
    let max_y = TRAY_Y + TRAY_H - DIE_SIZE;

    // Clone intersection lists upfront to avoid simultaneous borrows
    let all_intersections: Vec<Vec<DefaultKey>> = ds.rolling_dice.iter().map(|rd| {
        state.entities.entities.get(rd.entity)
            .and_then(|e| e.get_component::<Collision2d>())
            .and_then(|c| c.intersections.as_ref().map(|v| v.clone()))
            .unwrap_or_default()
    }).collect();

    let n = ds.rolling_dice.len();
    for i in 0..n {
        if ds.rolling_dice[i].result.is_some() { continue; }
        for other_key in &all_intersections[i] {
            if ds.wall_keys[0] == Some(*other_key) {        // left wall
                ds.rolling_dice[i].vx = ds.rolling_dice[i].vx.abs();
                ds.rolling_dice[i].x  = ds.rolling_dice[i].x.max(TRAY_X);
            } else if ds.wall_keys[1] == Some(*other_key) { // right wall
                ds.rolling_dice[i].vx = -ds.rolling_dice[i].vx.abs();
                ds.rolling_dice[i].x  = ds.rolling_dice[i].x.min(max_x);
            } else if ds.wall_keys[2] == Some(*other_key) { // top wall
                ds.rolling_dice[i].vy = ds.rolling_dice[i].vy.abs();
                ds.rolling_dice[i].y  = ds.rolling_dice[i].y.max(TRAY_Y);
            } else if ds.wall_keys[3] == Some(*other_key) { // bottom wall
                ds.rolling_dice[i].vy = -ds.rolling_dice[i].vy.abs();
                ds.rolling_dice[i].y  = ds.rolling_dice[i].y.min(max_y);
            } else {
                // Die-die: process each pair once (j > i)
                if let Some(j) = ds.rolling_dice.iter().position(|rd| rd.entity == *other_key) {
                    if j > i && ds.rolling_dice[j].result.is_none() {
                        let x1 = ds.rolling_dice[i].x; let y1 = ds.rolling_dice[i].y;
                        let x2 = ds.rolling_dice[j].x; let y2 = ds.rolling_dice[j].y;
                        let dx = x1 - x2; let dy = y1 - y2;
                        let overlap_x = DIE_SIZE - dx.abs();
                        let overlap_y = DIE_SIZE - dy.abs();
                        if overlap_x > 0.0 && overlap_y > 0.0 {
                            if overlap_x < overlap_y {
                                let push = overlap_x / 2.0 + 0.5;
                                if dx >= 0.0 { ds.rolling_dice[i].x += push; ds.rolling_dice[j].x -= push; }
                                else         { ds.rolling_dice[i].x -= push; ds.rolling_dice[j].x += push; }
                                let vx1 = ds.rolling_dice[i].vx; let vx2 = ds.rolling_dice[j].vx;
                                ds.rolling_dice[i].vx = vx2; ds.rolling_dice[j].vx = vx1;
                            } else {
                                let push = overlap_y / 2.0 + 0.5;
                                if dy >= 0.0 { ds.rolling_dice[i].y += push; ds.rolling_dice[j].y -= push; }
                                else         { ds.rolling_dice[i].y -= push; ds.rolling_dice[j].y += push; }
                                let vy1 = ds.rolling_dice[i].vy; let vy2 = ds.rolling_dice[j].vy;
                                ds.rolling_dice[i].vy = vy2; ds.rolling_dice[j].vy = vy1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Sync corrected positions back to Transform so the render shows final state
    for rd in &ds.rolling_dice {
        if rd.result.is_some() { continue; }
        if let Some(entity) = state.entities.entities.get_mut(rd.entity) {
            if let Some(tf) = entity.get_component_mut::<Transform>() {
                tf.set_x(rd.x);
                tf.set_y(rd.y);
            }
        }
    }

    // Settle check — after collision response so reflected velocities are included
    let settle_threshold = ds.material.settle_threshold();
    for rd in &mut ds.rolling_dice {
        if rd.result.is_some() { continue; }
        let speed = (rd.vx.powi(2) + rd.vy.powi(2)).sqrt();
        if speed < settle_threshold {
            rd.result = Some(do_roll(rd.die_type, rd.sides));
        }
    }
    if !ds.results_shown && ds.rolling_dice.iter().all(|rd| rd.result.is_some()) {
        show_results(state, ds);
    }
}

fn show_results(state: &mut GameState, ds: &mut DiceState) {
    ds.results_shown = true;

    let is_percentile = ds.rolling_dice.iter().any(|rd| rd.die_type == DieType::PercentileTens);

    if is_percentile {
        let tens  = ds.rolling_dice.iter().find(|rd| rd.die_type == DieType::PercentileTens).and_then(|rd| rd.result).unwrap_or(0);
        let ones  = ds.rolling_dice.iter().find(|rd| rd.die_type == DieType::PercentileOnes).and_then(|rd| rd.result).unwrap_or(0);
        let total = if tens == 0 && ones == 0 { 100 } else { tens + ones };
        let white = Color { r: 220, g: 220, b: 255, a: 255 };

        let start_y = centered_result_start(2, true);
        let lines_y = start_y + 36;
        let sep_y   = lines_y + 2 * RESULT_LINE_H + 6;
        let total_y = sep_y + 10;

        place_result_widget(state, ds.result_title_key,    start_y,               "d%",                    result_title_styles());
        place_result_line(  state, ds.result_line_keys[0], lines_y,               &format!("{:02}", tens), white.clone());
        place_result_line(  state, ds.result_line_keys[1], lines_y + RESULT_LINE_H, &ones.to_string(),     white);
        place_result_widget(state, ds.result_sep_key,      sep_y,                 "",                      result_sep_styles());
        place_result_widget(state, ds.result_total_key,    total_y, &total.to_string(), result_total_styles_colored(result_color(total, 100)));
        return;
    }

    let values: Vec<u32> = ds.rolling_dice.iter().filter_map(|rd| rd.result).collect();
    if values.is_empty() { return; }

    let sides = ds.rolling_dice[0].sides;
    let n     = values.len() as u32;
    let sum: u32 = values.iter().sum();
    let has_total = n > 1;

    let start_y = centered_result_start(n, has_total);
    let lines_y = start_y + 36;

    let title = if n == 1 { format!("d{}", sides) } else { format!("{}d{}", n, sides) };
    place_result_widget(state, ds.result_title_key, start_y, &title, result_title_styles());

    for (i, &val) in values.iter().enumerate() {
        let line_y = lines_y + i as u32 * RESULT_LINE_H;
        place_result_line(state, ds.result_line_keys[i], line_y, &val.to_string(), result_color(val, sides));
    }

    if has_total {
        let sep_y   = lines_y + n * RESULT_LINE_H + 6;
        let total_y = sep_y + 10;
        place_result_widget(state, ds.result_sep_key,   sep_y,   "",              result_sep_styles());
        place_result_widget(state, ds.result_total_key, total_y, &sum.to_string(), result_total_styles_colored(result_color(sum, n * sides)));
    }
}

// Positions, labels, and styles a result widget in one call.
fn place_result_widget(state: &mut GameState, key: Option<DefaultKey>, y: u32, label: &str, styles: WidgetStyles) {
    if let Some(k) = key {
        if let Some(w) = state.widgets.get_mut(k) {
            w.position.y = y;
            w.label      = label.to_string();
            w.active     = true;
            w.styles     = styles;
        }
    }
}

fn place_result_line(state: &mut GameState, key: Option<DefaultKey>, y: u32, label: &str, font_color: Color) {
    place_result_widget(state, key, y, label, result_line_styles(font_color));
}

fn result_total_styles_colored(font_color: Color) -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 50, g: 30, b: 10, a: 240 },
        hover_color: None,
        font_color,
        font_hover_color: None,
    }
}

// Returns the y coordinate where the result block should start so it's
// vertically centred in the right panel below the material toggle.
fn centered_result_start(n_lines: u32, has_total: bool) -> u32 {
    let block_h = 36 + n_lines * RESULT_LINE_H + if has_total { 46 } else { 0 };
    const AVAIL_TOP: u32 = 65;   // just below material toggle
    const AVAIL_H:   u32 = 555;  // SCREEN_H(640) - AVAIL_TOP(65) - bottom_margin(20)
    AVAIL_TOP + AVAIL_H.saturating_sub(block_h) / 2
}

fn hide_results(state: &mut GameState, ds: &DiceState) {
    let singles = [ds.result_title_key, ds.result_sep_key, ds.result_total_key];
    for key in singles.iter().chain(ds.result_line_keys.iter()) {
        if let Some(k) = key {
            if let Some(w) = state.widgets.get_mut(*k) {
                w.label  = String::new();
                w.active = false;
                w.styles = hidden_styles();
            }
        }
    }
}
