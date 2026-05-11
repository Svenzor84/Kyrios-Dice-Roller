#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kyrios::game::{Game, GameLogic, GameState, MapStyle};
use kyrios::components::Sprite;
use kyrios::types::{Point2d, Transform};
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
// Fraction of velocity remaining after one full second — applied as powf(delta)
// so behaviour is identical at any framerate (60fps, 144fps, 240fps, etc.)
const DAMPING_PER_SEC: f32 = 0.35;
const SETTLE_THRESHOLD: f32 = 15.0;
const MAX_DICE: u32 = 8;

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

// ── Right panel: results (widgets) ───────────────────────────────────────────
const PANEL_X: u32 = 768;
const PANEL_W: u32 = 180;
const RESULT_TITLE_Y: u32 = 190;
const RESULT_VALUES_Y: u32 = 265;
const RESULT_TOTAL_Y: u32 = 340;

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

struct RollingDie {
    entity: DefaultKey,
    sides: u32,
    x: f32, y: f32,
    vx: f32, vy: f32,
    result: Option<u32>,
}

struct DiceState {
    app_state: AppState,

    // Drag / hold
    dragging: bool,
    drag_idx: usize,    // index into BTNS
    drag_count: u32,

    // Active rolling dice
    rolling_dice: Vec<RollingDie>,
    results_shown: bool,

    // Permanent entities
    tray_entity:    Option<DefaultKey>,
    btn_entities:   [Option<DefaultKey>; 7],
    cursor_entity:  Option<DefaultKey>,  // die sprite that follows the cursor

    // Widgets
    count_label_key:    Option<DefaultKey>,  // "2d6" badge near cursor
    result_title_key:   Option<DefaultKey>,  // "3d6"
    result_values_key:  Option<DefaultKey>,  // "4+2+5"
    result_total_key:   Option<DefaultKey>,  // "= 11"

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
            dragging: false, drag_idx: 0, drag_count: 1,
            rolling_dice: Vec::new(), results_shown: false,
            tray_entity: None, btn_entities: [None; 7], cursor_entity: None,
            count_label_key: None,
            result_title_key: None, result_values_key: None, result_total_key: None,
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

fn result_values_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 30, g: 30, b: 60, a: 210 },
        hover_color: None,
        font_color: Color { r: 230, g: 230, b: 255, a: 255 },
        font_hover_color: None,
    }
}

fn result_total_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 50, g: 30, b: 10, a: 240 },
        hover_color: None,
        font_color: Color { r: 255, g: 200, b: 60, a: 255 },
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

fn splash_inst_styles() -> WidgetStyles {
    WidgetStyles {
        color: Color { r: 25, g: 25, b: 70, a: 220 },
        hover_color: None,
        font_color: Color { r: 180, g: 200, b: 255, a: 255 },
        font_hover_color: None,
    }
}

// ── Roll logic ────────────────────────────────────────────────────────────────

fn roll_percentile() -> u32 {
    let tens = rand::thread_rng().gen_range(0..=9) * 10;
    let ones = rand::thread_rng().gen_range(0..=9);
    if tens == 0 && ones == 0 { 100 } else { tens + ones }
}

fn do_roll(sides: u32) -> u32 {
    if sides == 100 { roll_percentile() } else { rand::thread_rng().gen_range(1..=sides) }
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
        embed!("sprites/tray_bg.png");
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

    game.texture_list.insert("tray_bg".to_string(), assets.path("assets/sprites/tray_bg.png"));
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

    game.init();
    loop { if game.update() == -1 { break; } }
}

// ── Setup ─────────────────────────────────────────────────────────────────────

fn setup_ui(state: &mut GameState, ds: &mut DiceState) {
    // Tray — first entity so it always has the lowest slot index and renders behind all others
    let mut tray = Entity::new();
    tray.add_component(Sprite {
        name: "tray".to_string(), texture_id: "tray_bg".to_string(),
        width: 4, height: 4, x: 0, y: 0, animations: None, animation_index: 0,
    });
    let mut tf = Transform::new();
    tf.set_length(TRAY_W); tf.set_width(TRAY_H);
    tf.set_x(TRAY_X); tf.set_y(TRAY_Y);
    tray.add_component(tf);
    ds.tray_entity = Some(state.entities.entities.insert(tray));

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

    // Right panel: result title ("3d6")
    let rt = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: RESULT_TITLE_Y },
        width: PANEL_W, height: 55,
        visible: false, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    let rtk = state.widgets.insert(rt);
    state.widgets[root_key].children.push(rtk);
    ds.result_title_key = Some(rtk);

    // Right panel: result values ("4+2+5")
    let rv = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: RESULT_VALUES_Y },
        width: PANEL_W, height: 45,
        visible: false, active: false,
        styles: hidden_styles(),
        ..Default::default()
    };
    let rvk = state.widgets.insert(rv);
    state.widgets[root_key].children.push(rvk);
    ds.result_values_key = Some(rvk);

    // Right panel: result total ("= 11")
    let rto = Widget {
        label: String::new(),
        position: Point2d { x: PANEL_X, y: RESULT_TOTAL_Y },
        width: PANEL_W, height: 55,
        visible: false, active: false,
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

    // While dragging: track mouse, accumulate dice, release
    if ds.dragging {
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
    spawn_dice(state, ds, count, sides, tex_id);
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

fn spawn_dice(state: &mut GameState, ds: &mut DiceState, count: u32, sides: u32, tex_id: &'static str) {
    let positions = tray_spawn_positions(count);
    let mut rng = rand::thread_rng();

    for (px, py) in positions {
        let spd_x: f32 = rng.gen_range(500.0_f32..=1100.0);
        let spd_y: f32 = rng.gen_range(500.0_f32..=1100.0);
        let vx = if rng.gen_bool(0.5) { spd_x } else { -spd_x };
        let vy = if rng.gen_bool(0.5) { spd_y } else { -spd_y };

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

        let key = state.entities.entities.insert(die);
        ds.rolling_dice.push(RollingDie { entity: key, sides, x: px, y: py, vx, vy, result: None });
    }
}

fn tray_spawn_positions(count: u32) -> Vec<(f32, f32)> {
    let cx = TRAY_X + (TRAY_W - DIE_SIZE) / 2.0;
    let cy = TRAY_Y + (TRAY_H - DIE_SIZE) / 2.0;
    if count == 1 {
        return vec![(cx, cy)];
    }
    let radius = 80.0_f32.min(TRAY_W.min(TRAY_H) / 5.5);
    (0..count).map(|i| {
        let angle = (i as f32 / count as f32) * std::f32::consts::TAU;
        let px = (cx + angle.cos() * radius).max(TRAY_X).min(TRAY_X + TRAY_W - DIE_SIZE);
        let py = (cy + angle.sin() * radius).max(TRAY_Y).min(TRAY_Y + TRAY_H - DIE_SIZE);
        (px, py)
    }).collect()
}

fn update_rolling(state: &mut GameState, ds: &mut DiceState) {
    let delta = (state.tick.delta as f32 / 1_000_000.0).min(0.1);
    let damping = DAMPING_PER_SEC.powf(delta);
    let max_x = TRAY_X + TRAY_W - DIE_SIZE;
    let max_y = TRAY_Y + TRAY_H - DIE_SIZE;

    // Phase 1: individual physics (position + wall bounce + damping)
    for rd in &mut ds.rolling_dice {
        if rd.result.is_some() { continue; }

        rd.x += rd.vx * delta;
        rd.y += rd.vy * delta;

        if rd.x <= TRAY_X { rd.x = TRAY_X; rd.vx =  rd.vx.abs(); }
        if rd.x >= max_x  { rd.x = max_x;  rd.vx = -rd.vx.abs(); }
        if rd.y <= TRAY_Y { rd.y = TRAY_Y; rd.vy =  rd.vy.abs(); }
        if rd.y >= max_y  { rd.y = max_y;  rd.vy = -rd.vy.abs(); }

        rd.vx *= damping;
        rd.vy *= damping;
    }

    // Phase 2: die-die collision (AABB, elastic equal-mass)
    die_collisions(ds);

    // Phase 3: sync all positions to entity Transforms
    for rd in &mut ds.rolling_dice {
        if rd.result.is_some() { continue; }
        if let Some(entity) = state.entities.entities.get_mut(rd.entity) {
            if let Some(tf) = entity.get_component_mut::<Transform>() {
                tf.set_x(rd.x);
                tf.set_y(rd.y);
            }
        }
        let speed = (rd.vx.powi(2) + rd.vy.powi(2)).sqrt();
        if speed < SETTLE_THRESHOLD {
            rd.result = Some(do_roll(rd.sides));
        }
    }

    if !ds.results_shown && ds.rolling_dice.iter().all(|rd| rd.result.is_some()) {
        show_results(state, ds);
    }
}

fn die_collisions(ds: &mut DiceState) {
    let n = ds.rolling_dice.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if ds.rolling_dice[i].result.is_some() || ds.rolling_dice[j].result.is_some() {
                continue;
            }

            // Read values by copy (f32 is Copy) — avoids simultaneous mut borrows
            let x1 = ds.rolling_dice[i].x;  let y1 = ds.rolling_dice[i].y;
            let x2 = ds.rolling_dice[j].x;  let y2 = ds.rolling_dice[j].y;
            let dx = x1 - x2;
            let dy = y1 - y2;

            let overlap_x = DIE_SIZE - dx.abs();
            let overlap_y = DIE_SIZE - dy.abs();

            if overlap_x <= 0.0 || overlap_y <= 0.0 { continue; }  // no overlap

            if overlap_x < overlap_y {
                // Shallower overlap along x — resolve and bounce on that axis
                let push = overlap_x / 2.0 + 0.5;
                if dx >= 0.0 {
                    ds.rolling_dice[i].x += push;
                    ds.rolling_dice[j].x -= push;
                } else {
                    ds.rolling_dice[i].x -= push;
                    ds.rolling_dice[j].x += push;
                }
                let vx1 = ds.rolling_dice[i].vx;
                let vx2 = ds.rolling_dice[j].vx;
                ds.rolling_dice[i].vx = vx2;
                ds.rolling_dice[j].vx = vx1;
            } else {
                // Shallower overlap along y
                let push = overlap_y / 2.0 + 0.5;
                if dy >= 0.0 {
                    ds.rolling_dice[i].y += push;
                    ds.rolling_dice[j].y -= push;
                } else {
                    ds.rolling_dice[i].y -= push;
                    ds.rolling_dice[j].y += push;
                }
                let vy1 = ds.rolling_dice[i].vy;
                let vy2 = ds.rolling_dice[j].vy;
                ds.rolling_dice[i].vy = vy2;
                ds.rolling_dice[j].vy = vy1;
            }
        }
    }
}

fn show_results(state: &mut GameState, ds: &mut DiceState) {
    ds.results_shown = true;

    let values: Vec<u32> = ds.rolling_dice.iter().filter_map(|rd| rd.result).collect();
    if values.is_empty() { return; }

    let sides = ds.rolling_dice[0].sides;
    let n = values.len();
    let sum: u32 = values.iter().sum();

    // Title: "d6", "3d6", "d%"
    let title = if n == 1 {
        if sides == 100 { "d%".to_string() } else { format!("d{}", sides) }
    } else {
        format!("{}d{}", n, sides)
    };

    // Values: individual results for ≤4 dice, compact summary for 5-8
    let vals_str = if n <= 4 {
        values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("+")
    } else {
        format!("({} dice)", n)
    };

    // Total: shown only for multi-die rolls
    let total_str = if n > 1 { format!("= {}", sum) } else { String::new() };

    set_result_widget(state, ds.result_title_key, &title, result_title_styles());
    set_result_widget(state, ds.result_values_key, &vals_str, result_values_styles());
    if !total_str.is_empty() {
        set_result_widget(state, ds.result_total_key, &total_str, result_total_styles());
    }
}

fn set_result_widget(state: &mut GameState, key: Option<DefaultKey>, label: &str, styles: WidgetStyles) {
    if let Some(k) = key {
        if let Some(w) = state.widgets.get_mut(k) {
            w.label  = label.to_string();
            w.active = true;
            w.styles = styles;
        }
    }
}

fn hide_results(state: &mut GameState, ds: &DiceState) {
    for key in [ds.result_title_key, ds.result_values_key, ds.result_total_key] {
        if let Some(k) = key {
            if let Some(w) = state.widgets.get_mut(k) {
                w.label  = String::new();
                w.active = false;
                w.styles = hidden_styles();
            }
        }
    }
}
