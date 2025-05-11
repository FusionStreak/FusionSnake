use rocket::serde::{Deserialize, Serialize};
use std::string::String;

/// Royale settings object
///
/// This object contains the shrink every n turns of the royale settings.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > RuleSettings](https://docs.battlesnake.com/api/objects/ruleset-settings)
///
/// # Attributes
///
/// * `shrink_every_n_turns` - In Royale mode, the number of turns between generating new hazards (shrinking the safe board space).
///
/// # Examples
///
/// ```
/// use game_objects::RoyaleSettings;
///
/// let royale_settings = RoyaleSettings {
///    shrink_every_n_turns: 10,
/// };
/// ```
#[derive(Deserialize, Serialize, Debug)]
pub struct RoyaleSettings {
    #[serde(rename = "shrinkEveryNTurns")]
    /// The number of turns between generating new hazards (shrinking the safe board space).
    pub(super) shrink_every_nturns: u32,
}

/// Squad settings object
///
/// This object contains the allow body collisions, shared elimination, shared health, and shared length of the squad settings.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > RuleSettings](https://docs.battlesnake.com/api/objects/ruleset-settings)
///
/// # Attributes
///
/// * `allow_body_collisions` - In Squad mode, allow members of the same squad to move over each other without dying.
/// * `shared_elimination` - In Squad mode, all squad members are eliminated when one is eliminated.
/// * `shared_health` - In Squad mode, all squad members share health.
/// * `shared_length` - In Squad mode, all squad members share length.
///
/// # Examples
/// ```
/// use game_objects::SquadSettings;
///
/// let squad_settings = SquadSettings {
///     allow_body_collisions: true,
///     shared_elimination: true,
///     shared_health: true,
///     shared_length: true,
/// };
/// ```
#[derive(Deserialize, Serialize, Debug)]

pub struct SquadSettings {
    #[serde(rename = "allowBodyCollisions")]
    pub(super) allow_body_collisions: bool,
    #[serde(rename = "sharedElimination")]
    pub(super) shared_elimination: bool,
    #[serde(rename = "sharedHealth")]
    pub(super) shared_health: bool,
    #[serde(rename = "sharedLength")]
    pub(super) shared_length: bool,
}

#[derive(Deserialize, Serialize, Debug)]

/// Ruleset settings object
///
/// This object contains the food spawn chance, minimum food, hazard damage per turn, royale settings, and squad settings of the ruleset settings.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > RuleSettings](https://docs.battlesnake.com/api/objects/ruleset-settings)
///
/// # Attributes
///
/// * `food_spawn_chance` - The chance of spawning food on any open tile on the board.
/// * `minimum_food` - The minimum number of food items that will be present on the board at any given time.
/// * `hazard_damage_per_turn` - The amount of damage that hazards will deal to snakes each turn.
/// * `royale` - Royale settings of the ruleset settings.
/// * `squad` - Squad settings of the ruleset settings.
///
/// # Examples
///
/// ```
/// use game_objects::RulesetSettings;
/// use game_objects::RoyaleSettings;
/// use game_objects::SquadSettings;
///
/// let royale_settings = RoyaleSettings {
///     shrink_every_n_turns: 10,
/// };
///
/// let squad_settings = SquadSettings {
///     allow_body_collisions: true,
///     shared_elimination: true,
///     shared_health: true,
///     shared_length: true,
/// };
///
/// let ruleset_settings = RulesetSettings {
///     food_spawn_chance: 15,
///     minimum_food: 1,
///     hazard_damage_per_turn: 15,
///     royale: royale_settings,
///     squad: squad_settings,
/// };
/// ```
pub struct RulesetSettings {
    #[serde(rename = "foodSpawnChance")]
    pub(super) food_spawn_chance: u32,
    #[serde(rename = "minimumFood")]
    pub(super) minimum_food: u32,
    #[serde(rename = "hazardDamagePerTurn")]
    pub(super) hazard_damage_per_turn: u32,
    pub(super) royale: RoyaleSettings,
    pub(super) squad: SquadSettings,
}

/// Ruleset object
///
/// This object contains the name, version, and settings of the ruleset.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > Ruleset](https://docs.battlesnake.com/api/objects/ruleset)
///
/// # Attributes
///
/// * `name` - Name of the ruleset being used to run this game. Example: "standard"
/// * `version` - The release version of the [Rules](https://github.com/BattlesnakeOfficial/rules) module used in this game. Example: "version": "v1.2.3"
/// * `settings` - A collection of [specific settings](https://docs.battlesnake.com/api/objects/ruleset-settings) being used by the current game that control how the rules are applied.
///
/// # Examples
///
/// ```
/// use game_objects::Ruleset;
/// use game_objects::RulesetSettings;
///
/// let ruleset_settings = RulesetSettings {
///     food_spawn_chance: 15,
///     minimum_food: 1,
///     hazard_damage_per_turn: 15,
///     royale: royale_settings,
///     squad: squad_settings,
/// };
///
/// let ruleset = Ruleset {
///     name: "standard".to_string(),
///     version: "v1".to_string(),
///     settings: ruleset_settings,
/// };
/// ```
#[derive(Deserialize, Serialize, Debug)]

pub struct Ruleset {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) settings: RulesetSettings,
}

/// Game object
///
/// This object contains the ID of the game, the ruleset of the game, and the timeout of the game.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > Game](https://docs.battlesnake.com/api/objects/game)
///
/// # Attributes
///
/// * `id` - A unique identifier for this Game. Example: "totally-unique-game-id"
/// * `ruleset` - Information about the ruleset being used to run this game. Example: {"name": "standard", "version": "v1.2.3"}
/// * `timeout` - How much time your snake has to respond to requests for this Game. Example: 500
/// * `map` - The name of the map being used for this game. Example: "standard"
/// * `source` - The source of the game. One of:
///     * "torunament" - The game is part of a tournament.
///     * "league" - for League Arenas.
///     * " arena" - for all other Arenas.
///     * "challenge" - for games created by a challenge.
///     * "custom" - for all other games.
///
/// # Examples
///
/// ```
/// use rocket::serde::json::Json;
/// use serde_json::Value;
/// use std::collections::HashMap;
///
/// use game_objects::Game;
///
/// let ruleset: HashMap<String, Value> = HashMap::new();
/// let game = Game::new("game-id".to_string(), ruleset, 500, "standard".to_string(), "custom".to_string());
/// ```
#[derive(Deserialize, Serialize, Debug)]
pub struct Game {
    pub(super) id: String,
    pub(super) ruleset: Ruleset,
    pub(super) timeout: u32,
    pub(super) map: String,
    pub(super) source: String,
}

/// Board object
///
/// This object contains the height, width, food, snakes, and hazards of the board.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > Board](https://docs.battlesnake.com/api/objects/board)
///
/// # Attributes
///
/// * `height` - The number of rows in the y-axis of the game board. Example: 11
/// * `width` - The number of columns in the x-axis of the game board. Example: 11
/// * `food` - Array of coordinates representing food locations on the game board. Example: [{"x": 5, "y": 5}, ..., {"x": 2, "y": 6}]
/// * `snakes` - Array of coordinates representing hazardous locations on the game board. Example: [{"x": 0, "y": 0}, ..., {"x": 0, "y": 1}]
/// * `hazards` - Array of [Battlesnake Objects](https://docs.battlesnake.com/api/objects/battlesnake) representing all Battlesnakes remaining on the game board (including yourself if you haven't been eliminated). Example: [{"id": "snake-one", ...}, ...]
///
/// # Example
///
/// ```json
/// {
///   "height": 11,
///   "width": 11,
///   "food": [
///     {"x": 5, "y": 5},
///     {"x": 9, "y": 0},
///     {"x": 2, "y": 6}
///   ],
///   "hazards": [
///     {"x": 0, "y": 0},
///     {"x": 0, "y": 1},
///     {"x": 0, "y": 2}
///   ],
///   "snakes": [
///     {"id": "snake-one", ... },
///     {"id": "snake-two", ... },
///     {"id": "snake-three", ... }
///   ]
/// }
/// ```
#[derive(Deserialize, Serialize, Debug)]
pub struct Board {
    pub(super) height: i32,
    pub(super) width: i32,
    pub(super) food: Vec<Coord>,
    pub(super) snakes: Vec<Battlesnake>,
    pub(super) hazards: Vec<Coord>,
}

/// Customization object
///
/// This object contains the color, head, and tail of the customization.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > Battlesnake](https://docs.battlesnake.com/api/objects/battlesnake)
///
/// # Attributes
///
/// * `color` - The color of the Battlesnake in hex format. Example: "#888888"
/// * `head` - The head of the Battlesnake. Example: "default"
/// * `tail` - The tail of the Battlesnake. Example: "default"
#[derive(Deserialize, Serialize, Debug)]
pub struct Customization {
    pub(super) color: String,
    pub(super) head: String,
    pub(super) tail: String,
}

/// Battlesnake object
///
/// This object contains the id, name, health, body, head, length, latency, shout, squad, and customizations of the Battlesnake.
///
/// Defined in: [BattleSnakes Docs > API Reference > Objects > Battlesnake](https://docs.battlesnake.com/api/objects/battlesnake)
///
/// # Attributes
///
/// * `id` - Unique identifier for this Battlesnake in the context of the current Game.Example: "totally-unique-snake-id"
/// * `name` - Name given to this Battlesnake by its author.Example: "Sneky McSnek Face"
/// * `health` - Health value of this Battlesnake, between 0 and 100 inclusively.Example: 54
/// * `body` - Array of coordinates representing this Battlesnake's location on the game board. This array is ordered from head to tail.Example: [{"x": 0, "y": 0}, ..., {"x": 2, "y": 0}]
/// * `head` - Coordinates for this Battlesnake's head. Equivalent to the first element of the body array.Example: {"x": 0, "y": 0}
/// * `length` - Length of this Battlesnake from head to tail. Equivalent to the length of the body array.Example: 3
/// * `latency` - The previous response time of this Battlesnake, in milliseconds. If the Battlesnake timed out and failed to respond, the game timeout will be returned (game.timeout)Example: "500"
/// * `shout` - Message shouted by this Battlesnake on the previous turn.Example: "why are we shouting??"
/// * `squad` - The squad that the Battlesnake belongs to. Used to identify squad members in Squad Mode games.Example: "1"
/// * `customizations` - The collection of customizations that control how this Battlesnake is displayed. Example: {"color":"#888888", "head":"default", "tail":"default" }
///
/// # Example
///
/// ```json
/// {
///   "id": "totally-unique-snake-id",
///   "name": "Sneky McSnek Face",
///   "health": 54,
///   "body": [
///     {"x": 0, "y": 0},
///     {"x": 1, "y": 0},
///     {"x": 2, "y": 0}
///   ],
///   "latency": "123",
///   "head": {"x": 0, "y": 0},
///   "length": 3,
///   "shout": "why are we shouting??",
///   "squad": "1",
///   "customizations":{
///     "color":"#26CF04",
///     "head":"smile",
///     "tail":"bolt"
///   }
/// }
/// ```
#[derive(Deserialize, Serialize, Debug)]
pub struct Battlesnake {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) health: u32,
    pub(super) body: Vec<Coord>,
    pub(super) head: Coord,
    pub(super) length: u32,
    pub(super) latency: String,
    pub(super) shout: Option<String>,
    pub(super) squad: Option<String>,
    pub(super) customizations: Customization,
}

/// Coord object
///
/// This object contains the x and y coordinates of the Coord.
///
/// # Attributes
///
/// * `x` - The x-coordinate of the Coord. Example: 5
/// * `y` - The y-coordinate of the Coord. Example: 5
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    pub(super) x: i32,
    pub(super) y: i32,
}

impl Coord {
    pub fn distance_to(&self, other: &Coord) -> u8 {
        ((self.x - other.x).abs() + (self.y - other.y).abs())
            .try_into()
            .unwrap()
    }
}
/// GameState object
///
/// This object contains the game, turn, board, and you of the GameState.
///
/// # Attributes
///
/// * `game` - The game object of the GameState.
/// * `turn` - The turn of the GameState.
/// * `board` - The board object of the GameState.
/// * `you` - The Battlesnake object of the GameState.
#[derive(Deserialize, Serialize, Debug)]
pub struct GameState {
    pub(super) game: Game,
    pub(super) turn: i32,
    pub(super) board: Board,
    pub(super) you: Battlesnake,
}
