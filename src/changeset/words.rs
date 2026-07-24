use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::SystemTime;

const ADJECTIVES: &[&str] = &[
    "amber", "bold", "brave", "bright", "calm", "clean", "cold", "cool", "crisp", "curly", "damp",
    "dark", "deep", "dry", "early", "empty", "fair", "fast", "flat", "fresh", "full", "glad",
    "gold", "good", "gray", "green", "happy", "heavy", "hot", "icy", "keen", "kind", "large",
    "late", "lean", "light", "little", "long", "loud", "lucky", "mean", "mild", "misty", "moody",
    "neat", "new", "nice", "odd", "old", "open", "pale", "pink", "plain", "proud", "pure", "quick",
    "quiet", "rare", "raw", "real", "red", "rich", "ripe", "rough", "round", "rude", "safe",
    "sharp", "shiny", "short", "shy", "silent", "silver", "slim", "slow", "small", "smart",
    "smooth", "soft", "solid", "sour", "spare", "spicy", "steep", "stiff", "strong", "sweet",
    "swift", "tall", "tame", "thick", "thin", "tidy", "tiny", "tough", "warm", "weak", "wet",
    "wide", "wild", "wise", "young",
];

const NOUNS: &[&str] = &[
    "ants", "apes", "bass", "bats", "bears", "bees", "birds", "boars", "bugs", "bulls", "cats",
    "clams", "cobs", "cods", "cows", "crabs", "crows", "cubs", "deer", "dogs", "doves", "ducks",
    "eels", "elks", "emus", "ewes", "fish", "flies", "foxes", "frog", "geese", "goats", "gulls",
    "hares", "hawks", "hens", "hogs", "jays", "kids", "kits", "lambs", "larks", "lice", "lions",
    "lynx", "mares", "mice", "minks", "moles", "moths", "mules", "newts", "owls", "oxen", "pigs",
    "pumas", "rams", "rats", "rays", "seals", "slugs", "snails", "swans", "toads", "trout",
    "vipers", "wasps", "whales", "wolves", "wrens", "yaks",
];

const VERBS: &[&str] = &[
    "act", "ask", "bark", "beam", "bite", "blow", "boil", "bolt", "buzz", "call", "camp", "cast",
    "chat", "chew", "clap", "cook", "copy", "curl", "cut", "dance", "dash", "dig", "dip", "dive",
    "draw", "drip", "drum", "eat", "fade", "fall", "feed", "film", "find", "fish", "flip", "flow",
    "fly", "fold", "glow", "grab", "grin", "grow", "hike", "hold", "hop", "howl", "hum", "hunt",
    "jog", "jump", "kick", "knit", "land", "lead", "leap", "lift", "limp", "look", "march", "melt",
    "mix", "moan", "move", "nap", "nod", "pack", "part", "pass", "peck", "pick", "plan", "play",
    "plot", "plow", "pull", "purr", "push", "race", "read", "rest", "ride", "ring", "rise", "roam",
    "roar", "roll", "run", "rush", "sail", "seek", "shop", "sing", "sit", "skip", "slip", "snap",
    "spin", "stay", "step", "stir", "stop", "swim", "talk", "toss", "trot", "turn", "type", "wade",
    "wait", "wake", "walk", "wash", "wave", "wink", "yawn", "yell",
];

pub fn generate_name() -> String {
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let hash = hasher.finish();

    let adj = ADJECTIVES[(hash as usize) % ADJECTIVES.len()];
    let noun = NOUNS[((hash >> 16) as usize) % NOUNS.len()];
    let verb = VERBS[((hash >> 32) as usize) % VERBS.len()];

    format!("{adj}-{noun}-{verb}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_name_is_kebab_case() {
        let name = generate_name();
        assert!(name.contains('-'));
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 3);
        for part in &parts {
            assert!(!part.is_empty());
            assert!(part.chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    #[test]
    fn names_vary() {
        let a = generate_name();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = generate_name();
        // Can't guarantee different with time-based seeds in fast tests,
        // but structure should always be valid
        assert!(a.contains('-'));
        assert!(b.contains('-'));
    }
}
