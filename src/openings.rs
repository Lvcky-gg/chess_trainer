//! Named opening theory, from the Lichess `chess-openings` database.
//!
//! Centipawn grading is the wrong instrument in the opening. A main line can
//! "lose" 20cp to an engine at depth 14 and still be the move every strong
//! player has made for a century, so calling it an inaccuracy teaches exactly
//! the wrong lesson. In book the useful verdict is not a number, it is a name
//! and a yes/no: *this is the Najdorf, and you are still in it*.
//!
//! Positions are keyed by Polyglot-compatible Zobrist hash rather than by move
//! sequence, so a line reached by transposition is recognised as the same
//! opening — which is most of the point of naming openings at all.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use shakmaty::san::SanPlus;
use shakmaty::zobrist::Zobrist64;
use shakmaty::{Chess, EnPassantMode, Move, Position};

/// eco, name, and the line as SAN with move numbers, one opening per line.
const OPENINGS_TSV: &str = include_str!("assets/openings.tsv");

/// How many alternative book moves to name when you leave theory. Some
/// positions have a dozen; a coach names two or three.
const MAX_CONTINUATIONS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    pub eco: String,
    pub name: String,
}

impl Opening {
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.eco)
    }
}

/// A subset of the database, selected by name or ECO code — the lines you are
/// actually trying to learn.
#[derive(Debug, Clone)]
pub struct Repertoire {
    /// Indices into `Book::openings`.
    lines: HashSet<u32>,
    /// What the user asked for, for display.
    pub label: String,
}

impl Repertoire {
    pub fn lines(&self) -> usize {
        self.lines.len()
    }

    /// A repertoire matching nothing, for exercising the empty-book paths.
    #[cfg(test)]
    pub fn empty(label: &str) -> Repertoire {
        Repertoire {
            lines: HashSet::new(),
            label: label.to_string(),
        }
    }
}

#[derive(Resource, Default)]
pub struct Book {
    openings: Vec<Opening>,
    /// Position -> the database lines that pass *through* it. The length of the
    /// list is what separates a main line from an obscure sideline: 1.e4 e5 is
    /// on hundreds of lines, the Bulgarian Variation on one. Without this the
    /// book can only say "named", never "usual", and a drill has no way to pick
    /// which move to teach.
    through: HashMap<u64, Vec<u32>>,
    /// Position -> the line that *ends* there. Walking the game forward and
    /// keeping the last hit lands on the most specific name, without having to
    /// compare line lengths.
    endings: HashMap<u64, u32>,
}

fn key(pos: &Chess) -> u64 {
    pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0
}

impl Book {
    /// Parse the embedded database. Malformed rows are skipped rather than
    /// fatal: a book with a hole in it is still worth having, and this must
    /// never be a reason the board fails to open.
    pub fn load() -> Book {
        let started = std::time::Instant::now();
        let mut book = Book::default();

        let mut skipped = 0;
        for line in OPENINGS_TSV.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let mut fields = line.split('\t');
            let (Some(eco), Some(name), Some(pgn)) =
                (fields.next(), fields.next(), fields.next())
            else {
                skipped += 1;
                continue;
            };

            let index = book.openings.len() as u32;
            match book.add_line(pgn, index) {
                Some(end) => {
                    book.openings.push(Opening {
                        eco: eco.to_string(),
                        name: name.to_string(),
                    });
                    book.endings.insert(end, index);
                }
                None => skipped += 1,
            }
        }

        if skipped > 0 {
            warn!("skipped {skipped} unreadable opening lines");
        }
        info!(
            "opening book: {} lines, {} positions in {:.0?}",
            book.openings.len(),
            book.through.len(),
            started.elapsed()
        );
        book
    }

    /// Replay one SAN line, recording every position along it as belonging to
    /// line `index`. Returns the key of the final position, or `None` if the
    /// line did not parse.
    ///
    /// A rejected line may already have registered some of its positions. That
    /// is harmless — those positions are real theory regardless — and cheaper
    /// than buffering every line to roll it back.
    fn add_line(&mut self, pgn: &str, index: u32) -> Option<u64> {
        let mut pos = Chess::default();
        self.through.entry(key(&pos)).or_default().push(index);

        for token in pgn.split_whitespace() {
            // "1." / "1..." are numbering, not moves.
            if token.ends_with('.') || token.chars().next()?.is_ascii_digit() {
                continue;
            }

            let san: SanPlus = token.parse().ok()?;
            let m = san.san.to_move(&pos).ok()?;
            pos = pos.play(m).ok()?;
            self.through.entry(key(&pos)).or_default().push(index);
        }

        Some(key(&pos))
    }

    pub fn is_empty(&self) -> bool {
        self.openings.is_empty()
    }

    /// Identifies a position, for callers that only want to know whether it
    /// changed. Cheaper than cloning or comparing the board.
    pub fn position_key(&self, pos: &Chess) -> u64 {
        key(pos)
    }

    /// Is this position still somewhere in known theory?
    pub fn contains(&self, pos: &Chess) -> bool {
        self.through.contains_key(&key(pos))
    }

    /// How many database lines run through this position. A rough stand-in for
    /// how mainstream it is — the database carries no popularity data.
    pub fn traffic(&self, pos: &Chess) -> usize {
        self.through.get(&key(pos)).map_or(0, Vec::len)
    }

    /// The opening this exact position is the endpoint of, if any.
    pub fn name(&self, pos: &Chess) -> Option<&Opening> {
        self.openings.get(*self.endings.get(&key(pos))? as usize)
    }

    /// Select the lines whose name contains `filter`, or whose ECO code equals
    /// it. `None` when nothing matched, so a typo is reported rather than
    /// silently drilling the whole database.
    pub fn repertoire(&self, filter: &str) -> Option<Repertoire> {
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }

        let lines: HashSet<u32> = self
            .openings
            .iter()
            .enumerate()
            .filter(|(_, opening)| {
                opening.name.to_lowercase().contains(&needle)
                    || opening.eco.eq_ignore_ascii_case(&needle)
            })
            .map(|(i, _)| i as u32)
            .collect();

        (!lines.is_empty()).then(|| Repertoire {
            lines,
            label: filter.trim().to_string(),
        })
    }

    /// Every book move from this position, most-travelled first, as
    /// (SAN, move, lines through the resulting position). Derived by trying each
    /// legal move rather than stored, which costs one hash per legal move and
    /// saves holding a second index of the whole book.
    fn ranked_moves(&self, pos: &Chess, within: Option<&Repertoire>) -> Vec<(String, Move, usize)> {
        let mut moves: Vec<(String, Move, usize)> = pos
            .legal_moves()
            .into_iter()
            .filter_map(|m| {
                let next = pos.clone().play(m).ok()?;
                let lines = self.through.get(&key(&next))?;

                // Inside a repertoire, only lines belonging to it count, so the
                // drill follows the chosen opening rather than the globally
                // most popular move.
                let weight = match within {
                    Some(rep) => lines.iter().filter(|i| rep.lines.contains(i)).count(),
                    None => lines.len(),
                };

                (weight > 0).then(|| (SanPlus::from_move(pos.clone(), m).to_string(), m, weight))
            })
            .collect();

        // Traffic first, then SAN, so the order is stable between runs.
        moves.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        moves
    }

    /// The moves that would have kept the game in theory, most-travelled first.
    pub fn continuations(&self, pos: &Chess) -> Vec<String> {
        let mut moves = self.ranked_moves(pos, None);
        moves.truncate(MAX_CONTINUATIONS);
        moves.into_iter().map(|(san, _, _)| san).collect()
    }

    /// The move to teach here: the most-travelled continuation, restricted to
    /// the repertoire being drilled. `None` once the lines run out.
    pub fn main_line(&self, pos: &Chess, within: Option<&Repertoire>) -> Option<(String, Move)> {
        let (san, m, _) = self.ranked_moves(pos, within).into_iter().next()?;
        Some((san, m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn book() -> &'static Book {
        static BOOK: OnceLock<Book> = OnceLock::new();
        BOOK.get_or_init(Book::load)
    }

    fn after(moves: &[&str]) -> Chess {
        let mut pos = Chess::default();
        for san in moves {
            let parsed: SanPlus = san.parse().expect("test move should parse");
            let m = parsed.san.to_move(&pos).expect("test move should be legal");
            pos = pos.play(m).expect("test move should be playable");
        }
        pos
    }

    #[test]
    fn the_database_loads_in_full() {
        // 3810 rows in the file, reaching ~7.9k distinct positions — far fewer
        // than 3810 lines x their length, because the lines share prefixes
        // heavily. Allow for the database being updated, but a large drop means
        // the parser has silently started rejecting lines.
        assert!(book().openings.len() > 3500, "{}", book().openings.len());
        assert!(book().through.len() > 7000, "{}", book().through.len());
    }

    #[test]
    fn the_starting_position_is_in_book() {
        assert!(book().contains(&Chess::default()));
    }

    #[test]
    fn a_main_line_is_named() {
        let sicilian = after(&["e4", "c5"]);
        let named = book().name(&sicilian).expect("1.e4 c5 should be named");
        assert!(named.name.contains("Sicilian"), "{}", named.name);
        assert_eq!(named.eco, "B20");
    }

    #[test]
    fn a_deeper_line_gets_the_more_specific_name() {
        let najdorf = after(&[
            "e4", "c5", "Nf3", "d6", "d4", "cxd4", "Nxd4", "Nf6", "Nc3", "a6",
        ]);
        let named = book().name(&najdorf).expect("the Najdorf should be named");
        assert!(named.name.contains("Najdorf"), "{}", named.name);
    }

    #[test]
    fn a_transposition_finds_the_same_position() {
        // 1.d4 Nf6 2.c4 e6 3.Nc3 and 1.c4 Nf6 2.Nc3 e6 3.d4 reach one position.
        let direct = after(&["d4", "Nf6", "c4", "e6", "Nc3"]);
        let transposed = after(&["c4", "Nf6", "Nc3", "e6", "d4"]);
        assert_eq!(key(&direct), key(&transposed));
        assert!(book().contains(&direct));
        assert!(book().contains(&transposed));
    }

    #[test]
    fn nonsense_leaves_the_book() {
        // 1.h4 a5 2.Rh3 is not theory anybody has bothered to name.
        assert!(!book().contains(&after(&["h4", "a5", "Rh3"])));
    }

    #[test]
    fn continuations_are_offered_from_a_book_position() {
        let moves = book().continuations(&Chess::default());
        assert!(!moves.is_empty());
        assert!(moves.len() <= MAX_CONTINUATIONS);
        // Every suggestion must itself be legal SAN in the position.
        for san in &moves {
            assert!(san.parse::<SanPlus>().is_ok(), "{san}");
        }
    }

    #[test]
    fn a_position_outside_the_book_offers_nothing_to_return_to() {
        // Deep in an unnamed line, no legal move leads back into theory.
        let lost = after(&["h4", "a5", "Rh3", "a4", "Rg3", "a3"]);
        assert!(book().continuations(&lost).is_empty());
    }

    #[test]
    fn traffic_separates_a_main_line_from_a_sideline() {
        let main = after(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        let sideline = after(&["e4", "e5", "Nf3", "Nc6", "Bb5", "a5"]);

        // The Bulgarian Variation is named, so `contains` cannot tell them
        // apart. Traffic can, and by a wide margin.
        assert!(book().contains(&sideline));
        assert!(
            book().traffic(&main) > 10 * book().traffic(&sideline),
            "main {} vs sideline {}",
            book().traffic(&main),
            book().traffic(&sideline)
        );
    }

    #[test]
    fn traffic_is_zero_outside_the_book() {
        assert_eq!(book().traffic(&after(&["h4", "a5", "Rh3"])), 0);
    }

    #[test]
    fn continuations_lead_with_the_most_travelled_move() {
        let ruy = after(&["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        let moves = book().continuations(&ruy);
        // 3...a6 is the Main Line; a5 and the rest are sidelines.
        assert_eq!(moves.first().map(String::as_str), Some("a6"), "{moves:?}");
    }

    #[test]
    fn a_repertoire_can_be_chosen_by_name_or_eco() {
        let by_name = book().repertoire("najdorf").expect("Najdorf lines exist");
        assert!(by_name.lines() > 5, "{}", by_name.lines());

        let by_eco = book().repertoire("B90").expect("B90 exists");
        assert!(by_eco.lines() > 0);

        // A typo selects nothing rather than silently drilling everything.
        assert!(book().repertoire("nadjorf").is_none());
        assert!(book().repertoire("   ").is_none());
    }

    #[test]
    fn a_repertoire_steers_the_move_that_gets_taught() {
        let start = Chess::default();

        // Globally, 1.e4 or 1.d4 lead. Asked for the English, the book teaches
        // the move that actually reaches it.
        let english = book().repertoire("English Opening").unwrap();
        let (san, _) = book()
            .main_line(&start, Some(&english))
            .expect("the English starts somewhere");
        assert_eq!(san, "c4");

        let dutch = book().repertoire("Dutch Defense").unwrap();
        let after_d4 = after(&["d4"]);
        let (san, _) = book().main_line(&after_d4, Some(&dutch)).unwrap();
        assert_eq!(san, "f5");
    }

    #[test]
    fn a_line_runs_out_at_its_end() {
        let english = book().repertoire("English Opening").unwrap();
        // Twenty plies of the English is past the end of any named line.
        let deep = after(&[
            "c4", "e5", "Nc3", "Nf6", "Nf3", "Nc6", "e3", "Bb4", "Qc2", "Bxc3",
            "Qxc3", "O-O", "Be2", "Re8", "O-O", "d5", "cxd5", "Nxd5", "Qc2", "Nb6",
        ]);
        assert!(book().main_line(&deep, Some(&english)).is_none());
    }

    #[test]
    fn an_unparseable_line_is_skipped_rather_than_fatal() {
        let mut book = Book::default();
        assert!(book.add_line("1. e4 zz9", 0).is_none());
        assert!(book.add_line("1. e4 e5 2. Ke2", 1).is_some());
    }

    #[test]
    fn move_numbers_are_not_mistaken_for_moves() {
        let mut book = Book::default();
        let plain = book.add_line("1. e4 e5 2. Nf3", 0);
        let mut other = Book::default();
        let numbered = other.add_line("1. e4 1... e5 2. Nf3", 0);
        assert_eq!(plain, numbered);
        assert!(plain.is_some());
    }

    #[test]
    fn a_named_opening_reads_as_a_label() {
        let opening = Opening {
            eco: "B90".to_string(),
            name: "Sicilian Defense: Najdorf Variation".to_string(),
        };
        assert_eq!(
            opening.label(),
            "Sicilian Defense: Najdorf Variation (B90)"
        );
    }
}
