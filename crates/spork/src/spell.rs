//! Spell check of commit messages (docs/specs/M07 §5): words found neither in the system word
//! list, nor in this repository's commit subjects and paths, nor in the user's own list.
// shortcut: hints under the message, not squiggles (the text field can't draw them); the system
// word list (/usr/share/dict/words) only, so it is off where there is none (Windows).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::repos;
use crate::util::ResultExt as _;

/// The system word list, compact: every word lowercased, sorted and joined in one string, with
/// where each starts. About 3 MB for 235 000 words, where a set of strings took 28 MB.
struct Words {
    text: String,
    starts: Vec<u32>,
}

impl Words {
    fn new(list: &str) -> Self {
        let mut words: Vec<String> = list.lines().map(str::to_lowercase).collect();
        words.sort_unstable();
        words.dedup();
        let (mut text, mut starts) = (String::new(), Vec::with_capacity(words.len()));
        for w in &words {
            starts.push(text.len() as u32);
            text.push_str(w);
        }
        starts.push(text.len() as u32);
        Self { text, starts }
    }

    fn word(&self, i: usize) -> &str {
        &self.text[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    fn contains(&self, word: &str) -> bool {
        let (mut lo, mut hi) = (0, self.starts.len() - 1);
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.word(mid).cmp(word) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
}

static SYSTEM: OnceLock<Option<Words>> = OnceLock::new();
static LOADING: std::sync::Once = std::sync::Once::new();
static MINE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// The word list once it has loaded. The first call starts loading it off the UI thread and
/// answers `None`: nothing is held for people who never write a commit message here.
fn system() -> Option<&'static Words> {
    LOADING.call_once(|| {
        std::thread::spawn(|| {
            SYSTEM.get_or_init(|| {
                let text = std::fs::read_to_string("/usr/share/dict/words").ok()?;
                Some(Words::new(&text))
            });
        });
    });
    SYSTEM.get()?.as_ref()
}

fn my_words_file() -> Option<std::path::PathBuf> {
    Some(repos::config_dir()?.join("words.txt"))
}

fn with_mine<T>(f: impl FnOnce(&mut HashSet<String>) -> T) -> Option<T> {
    let mut mine = MINE.lock().ok()?;
    let words = mine.get_or_insert_with(|| {
        my_words_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|t| t.lines().map(str::to_lowercase).collect())
            .unwrap_or_default()
    });
    Some(f(words))
}

/// Remembers `word` as spelt right, for every repository.
pub(crate) fn learn(word: &str) {
    let saved = with_mine(|words| {
        words.insert(word.to_lowercase());
        let mut list: Vec<&String> = words.iter().collect();
        list.sort();
        list.into_iter()
            .map(|w| format!("{w}\n"))
            .collect::<String>()
    });
    if let (Some(text), Some(file)) = (saved, my_words_file()) {
        std::fs::create_dir_all(file.parent().unwrap_or(&file)).log_err();
        std::fs::write(file, text).log_err();
    }
}

/// The words of `text` worth checking: letters only, four or more, not code-like (`camelCase`,
/// `ALLCAPS`, `snake_case` and words with digits are skipped).
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\''))
        .map(|w| w.trim_matches('\''))
        .filter(|w| w.len() >= 4 && w.chars().all(|c| c.is_alphabetic() || c == '\''))
        .filter(|w| !w.chars().skip(1).any(char::is_uppercase))
}

/// Vocabulary of a repository: the words of its commit subjects and file paths.
pub(crate) fn vocabulary<'a>(texts: impl Iterator<Item = &'a str>) -> HashSet<String> {
    texts
        .flat_map(|t| t.split(|c: char| !c.is_alphabetic()))
        .filter(|w| w.len() >= 4)
        .map(str::to_lowercase)
        .collect()
}

/// `word` (lowercase) or a plain stem of it is in `known`: fixes → fix, merged → merge,
/// dependencies → dependency, stopped → stop.
fn known_word(word: &str, known: &dyn Fn(&str) -> bool) -> bool {
    if known(word) {
        return true;
    }
    let stems = [
        ("ies", "y"),
        ("ied", "y"),
        ("es", ""),
        ("s", ""),
        ("ed", ""),
        ("ed", "e"),
        ("d", ""),
        ("ing", ""),
        ("ing", "e"),
        ("er", ""),
        ("ers", ""),
        ("ly", ""),
    ];
    stems.iter().any(|(suffix, add)| {
        let Some(stem) = word.strip_suffix(suffix) else {
            return false;
        };
        let stem = format!("{stem}{add}");
        // a doubled last letter: stopped → stopp → stop
        let mut last = stem.char_indices().rev();
        let undoubled = match (last.next(), last.next()) {
            (Some((cut, a)), Some((_, b))) if a == b => Some(stem[..cut].to_owned()),
            _ => None,
        };
        stem.len() >= 3 && (known(&stem) || undoubled.is_some_and(|u| known(&u)))
    })
}

/// The misspelt words of `text`, each once, in order; empty without a system word list.
pub(crate) fn typos(text: &str, repo_words: &HashSet<String>) -> Vec<String> {
    let Some(system) = system() else {
        return Vec::new();
    };
    let mine = with_mine(|w| w.clone()).unwrap_or_default();
    check(text, &|w| {
        system.contains(w) || repo_words.contains(w) || mine.contains(w)
    })
}

fn check(text: &str, known: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words(text) {
        let lower = word.to_lowercase();
        if !known_word(&lower, known) && !out.contains(&word.to_owned()) {
            out.push(word.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{Words, check, vocabulary};

    #[test]
    fn compact_word_list() {
        let words = Words::new("Apple\nzoo\napple\nbanana\n");
        assert!(words.contains("apple") && words.contains("banana") && words.contains("zoo"));
        assert!(!words.contains("app") && !words.contains("zzz") && !words.contains(""));
        assert_eq!(words.starts.len(), 4); // three words and the end; "Apple" folded into "apple"
    }

    #[test]
    fn flags_unknown_words_only() {
        let dict = [
            "fix",
            "merge",
            "dependency",
            "stop",
            "the",
            "receive",
            "login",
            "add",
        ];
        let repo = vocabulary(["refactor config loader"].into_iter());
        let known = |w: &str| dict.contains(&w) || repo.contains(w);
        assert_eq!(
            check(
                "Fixes merged dependencies, stopped adding teh login; recieve fooBar HTTP x2 a_b refactor",
                &known
            ),
            ["recieve"] // `teh` is too short to check; code-like words are skipped
        );
        assert_eq!(check("recieve recieve", &known), ["recieve"]);
    }
}
