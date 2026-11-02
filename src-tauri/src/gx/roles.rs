//! What a thing is (character, item, prop...) and its role (player, enemy,
//! npc), the same way for every engine. What it does counts first: a camera
//! inside it, a character controller, a body that moves on its own, a
//! pathfinding agent, colliders that only sense, copies around the level.
//! Its name breaks ties. Each reader fills in the signals it can see; the
//! reason is kept so the editor can say why.

use super::model::ThingKind;

/// What a reader saw of one object.
#[derive(Debug, Default, Clone)]
pub struct Signals {
    /// Its name (and its file's).
    pub name: String,
    /// Its scripts' names: they say what it is less surely ("PlayerDetector" on a door).
    pub scripts: String,
    /// A camera sits in it (the view goes where it goes).
    pub camera: bool,
    /// A character controller or character body: made to be walked around.
    pub controller: bool,
    /// A body physics moves (not one only scripts place).
    pub moves: bool,
    /// Finds its own way: a pathfinding agent, or AI scripts.
    pub agent: bool,
    /// Has colliders, and all of them only sense (triggers).
    pub trigger_only: bool,
    /// Animated frames of walking or running, or an animator.
    pub animated: bool,
    /// How many of it the level holds.
    pub copies: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Guess {
    pub kind: ThingKind,
    pub role: Option<String>,
    /// Why, in a few words ("the camera is inside it").
    pub why: String,
}

impl Guess {
    fn new(kind: ThingKind, role: Option<&str>, why: impl Into<String>) -> Guess {
        Guess { kind, role: role.map(String::from), why: why.into() }
    }

    /// "Player: the camera is inside it." for a thing's description.
    pub fn sentence(&self) -> String {
        let what = match (&self.role, self.kind) {
            (Some(r), _) => r.clone(),
            (None, ThingKind::Character) => "character".into(),
            (None, ThingKind::Item) => "item".into(),
            (None, ThingKind::Tool) => "tool".into(),
            (None, ThingKind::Prop) => "prop".into(),
            (None, ThingKind::Other) => "other".into(),
        };
        let mut s = what;
        if let Some(c) = s.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        format!("{s}: {}.", self.why)
    }
}

/// Words that mark a place that senses something, not the thing itself:
/// "PlayerSeeTrigger", "Check player floor", "EnemySpawnPoint".
const ZONE_WORDS: &[&str] = &[
    "trigger", "sensor", "check", "checker", "detector", "detect", "zone", "area", "region", "pos", "position", "point", "spawn",
    "spawner", "waypoint", "limit", "bounds", "boundary", "collider", "killzone", "marker",
];

pub fn classify(s: &Signals) -> Guess {
    let words = words_of(&s.name);
    let named = |list: &[&str]| words.iter().find(|w| list.contains(&w.as_str()) || list.iter().any(|l| w.as_str() == format!("{l}s"))).cloned();

    // Seen moving with the camera: the one you play, whatever it's called.
    if s.camera && (s.controller || s.moves) {
        let how = if s.controller { "a character controller" } else { "a body that moves" };
        return Guess::new(ThingKind::Character, Some("player"), format!("the camera is inside it and it has {how}"));
    }
    if let Some(w) = named(ZONE_WORDS) {
        if s.trigger_only || !(s.controller || s.moves || s.agent) {
            return Guess::new(ThingKind::Other, None, format!("a zone that senses things (\"{w}\" in its name)"));
        }
    }
    let alive = s.controller || s.moves || s.agent || s.animated;
    let (kind, role) = guess_kind(&s.name);
    // Scripts name the player or an enemy only on something that can move.
    let (kind, role, from) = match (kind, role) {
        (ThingKind::Other, None) => match guess_kind(&format!("{} {}", s.name, s.scripts)) {
            (_, Some(_)) if !alive => (ThingKind::Other, None, "its name"),
            (k, r) => (k, r, "its scripts"),
        },
        (k, r) => (k, r, "its name"),
    };
    if role.is_some() || kind != ThingKind::Other {
        let all = words_of(&format!("{} {}", s.name, s.scripts));
        let word = all.iter().find(|w| NAME_WORDS.contains(&w.as_str()) || NAME_WORDS.iter().any(|l| w.as_str() == format!("{l}s"))).cloned().unwrap_or_default();
        let mut g = Guess { kind, role, why: format!("{from} (\"{word}\")") };
        // A name says what it is; what it does says it's alive.
        if g.kind == ThingKind::Other && (s.controller || s.agent) {
            g.kind = ThingKind::Character;
        }
        return g;
    }
    if s.agent && (s.controller || s.moves || s.animated) {
        return Guess::new(ThingKind::Character, Some("enemy"), "it finds its own way (pathfinding or AI)");
    }
    if s.controller {
        return Guess::new(ThingKind::Character, None, "it has a character controller");
    }
    if s.moves && s.animated {
        return Guess::new(ThingKind::Character, None, "an animated body that moves");
    }
    if s.trigger_only && s.copies >= 2 {
        return Guess::new(ThingKind::Item, None, "several copies, each sensing touch");
    }
    if s.trigger_only {
        return Guess::new(ThingKind::Other, None, "a zone that senses things");
    }
    if s.moves {
        return Guess::new(ThingKind::Prop, None, "a body physics moves");
    }
    Guess::new(ThingKind::Other, None, "nothing about it says what it is")
}

/// Every word `guess_kind` listens for.
const NAME_WORDS: &[&str] = &[
    "gun", "weapon", "sword", "bullet", "tool", "projectile", "bomb", "coin", "gem", "key", "pickup", "collectible", "item", "potion", "heart",
    "powerup", "moneda", "llave", "pocion", "gema", "corazon", "platform", "door", "crate", "box", "wall", "block", "trap", "spike", "barrel",
    "chest", "lever", "switch", "enemy", "mob", "monster", "zombie", "bat", "slime", "ghost", "boss", "creep", "zombi", "enemigo", "monstruo",
    "fantasma", "murcielago", "player", "hero", "jugador", "heroe", "npc", "villager",
];

/// What a name alone says.
pub fn guess_kind(name: &str) -> (ThingKind, Option<String>) {
    let words = words_of(name);
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()) || list.iter().any(|l| w.as_str() == format!("{l}s")));
    // Screens and levels first: a "coins counter" is a label, not a coin.
    if has(&["menu", "hud", "ui", "gui", "button", "counter", "label", "background", "parallax", "level", "stage", "world", "music", "joystick"]) {
        return (ThingKind::Other, None);
    }
    // What it is beats who uses it: a "player bullet" is a bullet.
    if has(&["gun", "weapon", "sword", "bullet", "tool", "projectile", "bomb"]) {
        return (ThingKind::Tool, None);
    }
    if has(&["coin", "gem", "key", "pickup", "collectible", "item", "potion", "heart", "powerup", "moneda", "llave", "pocion", "gema", "corazon"]) {
        return (ThingKind::Item, None);
    }
    if has(&["platform", "door", "crate", "box", "wall", "block", "trap", "spike", "spikes", "barrel", "chest", "lever", "switch"]) {
        return (ThingKind::Prop, None);
    }
    if has(&["enemy", "mob", "monster", "zombie", "bat", "slime", "ghost", "boss", "creep", "zombi", "enemigo", "monstruo", "fantasma", "murcielago"]) {
        return (ThingKind::Character, Some("enemy".into()));
    }
    if has(&["player", "hero", "jugador", "heroe"]) {
        return (ThingKind::Character, Some("player".into()));
    }
    if has(&["npc", "villager"]) {
        return (ThingKind::Character, Some("npc".into()));
    }
    (ThingKind::Other, None)
}

/// "PlayerShip_2" -> ["player", "ship"].
pub fn words_of(name: &str) -> Vec<String> {
    let spaced: String = name.chars().fold(String::new(), |mut s, c| {
        if c.is_uppercase() && s.chars().last().is_some_and(|p| p.is_lowercase()) {
            s.push(' ');
        }
        s.push(if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
        s
    });
    spaced.split_whitespace().map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()).filter(|w| !w.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Signals {
        Signals { name: name.into(), ..Default::default() }
    }

    #[test]
    fn zones_named_after_the_player_are_not_the_player() {
        for name in ["GrannySeePlayerTrigger", "PlayerleaveTrigger", "Check player floor", "SpiderSensorPlayer", "PlayerSpawnPoint"] {
            let g = classify(&Signals { trigger_only: true, ..named(name) });
            assert_eq!((g.kind, g.role.as_deref()), (ThingKind::Other, None), "{name}");
        }
        assert_eq!(classify(&named("Player")).role.as_deref(), Some("player"));
    }

    #[test]
    fn what_it_does_beats_its_name() {
        // A first-person rig: no telling name, but the camera rides a controller.
        let g = classify(&Signals { camera: true, controller: true, ..named("FPSController") });
        assert_eq!((g.kind, g.role.as_deref()), (ThingKind::Character, Some("player")));
        assert!(g.why.contains("camera"));
        // A pathfinding agent that walks is an enemy, even named "Granny".
        let g = classify(&Signals { agent: true, animated: true, moves: true, ..named("Granny") });
        assert_eq!(g.role.as_deref(), Some("enemy"));
        // A trigger alone is a zone; several copies of one are pickups.
        assert_eq!(classify(&Signals { trigger_only: true, ..named("Thing") }).kind, ThingKind::Other);
        assert_eq!(classify(&Signals { trigger_only: true, copies: 6, ..named("Thing") }).kind, ThingKind::Item);
        // Something only scripts move (a kinematic door) isn't a character.
        assert_eq!(classify(&named("Galler")).kind, ThingKind::Other);
        assert_eq!(classify(&Signals { controller: true, ..named("Galler") }).kind, ThingKind::Character);
    }

    #[test]
    fn scripts_name_a_role_only_on_something_alive() {
        let window = Signals { scripts: "PlayerJumpOut".into(), ..named("JumpOutWindow") };
        assert_eq!(classify(&window).role, None);
        let rig = Signals { scripts: "PlayerMovement".into(), moves: true, ..named("Rig") };
        assert_eq!(classify(&rig).role.as_deref(), Some("player"));
        assert!(classify(&rig).why.starts_with("its scripts"));
    }

    #[test]
    fn says_why() {
        let g = classify(&Signals { camera: true, controller: true, ..named("Rig") });
        assert_eq!(g.sentence(), "Player: the camera is inside it and it has a character controller.");
        assert_eq!(classify(&named("GoldCoin")).sentence(), "Item: its name (\"coin\").");
    }
}
