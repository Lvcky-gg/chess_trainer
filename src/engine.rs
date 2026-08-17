use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use bevy::prelude::*;

const OPPONENT_MOVE_TIME_MS: u32 = 500;
const ANALYSIS_DEPTH: u32 = 14;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Score {
    Centipawns(i32),
    Mate(i32),
}

impl Score {
    pub fn to_cp(self) -> i32 {
        match self {
            Score::Centipawns(cp) => cp.clamp(-10_000, 10_000),
            Score::Mate(n) if n >= 0 => 10_000 - n.min(99) * 10,
            Score::Mate(n) => -10_000 - n.max(-99) * 10,
        }
    }

    pub fn label(self) -> String {
        match self {
            Score::Centipawns(cp) => format!("{:+.2}", cp as f32 / 100.0),
            Score::Mate(n) if n >= 0 => format!("M{n}"),
            Score::Mate(n) => format!("-M{}", n.abs()),
        }
    }
}

pub enum EngineRequest {
    Analyse { fen: String, ply: u32 },
    PlayMove { fen: String, ply: u32 },
    SetSkill(u32),
    Shutdown,
}

pub enum EngineReply {
    Analysis {
        ply: u32,
        score: Score,
        best_move: String,
    },
    OpponentMove {
        ply: u32,
        uci: String,
    },
    Unavailable(String),
}

#[derive(Resource)]
pub struct EngineLink {
    tx: Sender<EngineRequest>,
    rx: Mutex<Receiver<EngineReply>>,
    pub available: bool,
    pub status: String,
}

impl EngineLink {
    fn new(
        tx: Sender<EngineRequest>,
        rx: Receiver<EngineReply>,
        available: bool,
        status: String,
    ) -> Self {
        Self {
            tx,
            rx: Mutex::new(rx),
            available,
            status,
        }
    }

    pub fn send(&self, req: EngineRequest) {
        let _ = self.tx.send(req);
    }

    pub fn try_recv(&self) -> Option<EngineReply> {
        let rx = self.rx.lock().ok()?;
        rx.try_recv().ok()
    }
}

impl Drop for EngineLink {
    fn drop(&mut self) {
        let _ = self.tx.send(EngineRequest::Shutdown);
    }
}

pub fn discover_stockfish() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("STOCKFISH_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
        warn!(
            "STOCKFISH_PATH is set to {} but that is not a file",
            p.display()
        );
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("stockfish");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    [
        "/usr/bin/stockfish",
        "/usr/local/bin/stockfish",
        "/usr/games/stockfish",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

fn white_to_move(fen: &str) -> bool {
    fen.split_whitespace().nth(1) != Some("b")
}

struct Uci {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Uci {
    fn spawn(path: &std::path::Path) -> std::io::Result<Uci> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = BufReader::new(child.stdout.take().expect("stdout was piped"));

        let mut uci = Uci {
            child,
            stdin,
            stdout,
        };
        uci.send("uci")?;
        uci.wait_for("uciok")?;
        Ok(uci)
    }

    fn send(&mut self, cmd: &str) -> std::io::Result<()> {
        writeln!(self.stdin, "{cmd}")?;
        self.stdin.flush()
    }

    fn read_line(&mut self) -> std::io::Result<String> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "engine closed its output",
            ));
        }
        Ok(line.trim_end().to_string())
    }

    fn wait_for(&mut self, token: &str) -> std::io::Result<()> {
        loop {
            let line = self.read_line()?;
            if line.split_whitespace().next() == Some(token) {
                return Ok(());
            }
        }
    }

    fn is_ready(&mut self) -> std::io::Result<()> {
        self.send("isready")?;
        self.wait_for("readyok")
    }

    fn set_option(&mut self, name: &str, value: &str) -> std::io::Result<()> {
        self.send(&format!("setoption name {name} value {value}"))
    }

    fn new_game(&mut self) -> std::io::Result<()> {
        self.send("ucinewgame")?;
        self.is_ready()
    }

    fn search(&mut self, fen: &str, go_args: &str) -> std::io::Result<(Score, String)> {
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go {go_args}"))?;

        let sign = if white_to_move(fen) { 1 } else { -1 };
        let mut score = Score::Centipawns(0);

        loop {
            let line = self.read_line()?;
            let mut tokens = line.split_whitespace();

            match tokens.next() {
                Some("info") => {
                    if let Some(parsed) = parse_score(&line, sign) {
                        score = parsed;
                    }
                }
                Some("bestmove") => {
                    let best = tokens.next().unwrap_or("(none)").to_string();
                    return Ok((score, best));
                }
                _ => {}
            }
        }
    }

    fn quit(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.wait();
    }
}

fn parse_score(line: &str, sign: i32) -> Option<Score> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let i = tokens.iter().position(|t| *t == "score")?;
    let kind = tokens.get(i + 1)?;
    let value: i32 = tokens.get(i + 2)?.parse().ok()?;

    match *kind {
        "cp" => Some(Score::Centipawns(value * sign)),
        "mate" => Some(Score::Mate(value * sign)),
        _ => None,
    }
}

fn start_engine(path: &std::path::Path, skill: Option<u32>) -> std::io::Result<Uci> {
    let mut uci = Uci::spawn(path)?;
    if let Some(level) = skill {
        uci.set_option("Skill Level", &level.to_string())?;
    }
    uci.new_game()?;
    Ok(uci)
}

pub fn start(skill: u32) -> EngineLink {
    start_with(discover_stockfish(), skill)
}

/// A link that reports no engine, for tests that need the resource to exist
/// without touching Stockfish.
#[cfg(test)]
pub fn start_unavailable() -> EngineLink {
    start_with(None, 0)
}

/// Discovery is a parameter so that the not-found path stays testable. Probing
/// it through `start` would mean hiding every Stockfish on the machine — PATH
/// and `/usr/bin` included — which no environment variable can do.
fn start_with(path: Option<PathBuf>, skill: u32) -> EngineLink {
    let (req_tx, req_rx) = mpsc::channel::<EngineRequest>();
    let (rep_tx, rep_rx) = mpsc::channel::<EngineReply>();

    let Some(path) = path else {
        let msg =
            "Stockfish not found - install it (e.g. `yay -S stockfish`) or set STOCKFISH_PATH"
                .to_string();
        let _ = rep_tx.send(EngineReply::Unavailable(msg.clone()));
        return EngineLink::new(req_tx, rep_rx, false, msg);
    };

    std::thread::spawn(move || {
        let mut analyst = match start_engine(&path, None) {
            Ok(e) => e,
            Err(e) => {
                let _ = rep_tx.send(EngineReply::Unavailable(format!(
                    "could not start Stockfish at {}: {e}",
                    path.display()
                )));
                return;
            }
        };

        let mut opponent = match start_engine(&path, Some(skill)) {
            Ok(e) => e,
            Err(e) => {
                analyst.quit();
                let _ = rep_tx.send(EngineReply::Unavailable(format!(
                    "could not start the opponent engine: {e}"
                )));
                return;
            }
        };

        while let Ok(req) = req_rx.recv() {
            let outcome = match req {
                EngineRequest::Analyse { fen, ply } => analyst
                    .search(&fen, &format!("depth {ANALYSIS_DEPTH}"))
                    .map(|(score, best_move)| {
                        Some(EngineReply::Analysis {
                            ply,
                            score,
                            best_move,
                        })
                    }),

                EngineRequest::PlayMove { fen, ply } => opponent
                    .search(&fen, &format!("movetime {OPPONENT_MOVE_TIME_MS}"))
                    .map(|(_, uci)| Some(EngineReply::OpponentMove { ply, uci })),

                EngineRequest::SetSkill(level) => opponent
                    .set_option("Skill Level", &level.to_string())
                    .map(|()| None),

                EngineRequest::Shutdown => break,
            };

            match outcome {
                Ok(Some(reply)) => {
                    if rep_tx.send(reply).is_err() {
                        break; // Bevy side is gone
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = rep_tx.send(EngineReply::Unavailable(format!("engine failed: {e}")));
                    break;
                }
            }
        }

        analyst.quit();
        opponent.quit();
    });

    EngineLink::new(req_tx, rep_rx, true, "Stockfish ready".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_are_flipped_only_for_black_to_move() {
        assert_eq!(
            parse_score("info depth 12 score cp 120 pv e2e4", 1),
            Some(Score::Centipawns(120))
        );
        assert_eq!(
            parse_score("info depth 12 score cp 120 pv e7e5", -1),
            Some(Score::Centipawns(-120))
        );
    }

    #[test]
    fn mate_scores_are_parsed_and_ordered() {
        assert_eq!(
            parse_score("info depth 20 score mate 3 pv a1a8", 1),
            Some(Score::Mate(3))
        );
        assert_eq!(
            parse_score("info depth 20 score mate 3 pv a8a1", -1),
            Some(Score::Mate(-3))
        );
        assert!(Score::Mate(1).to_cp() > Score::Mate(5).to_cp());
        assert!(Score::Mate(5).to_cp() > Score::Centipawns(900).to_cp());
        assert!(Score::Mate(-1).to_cp() < Score::Mate(-5).to_cp());
        assert!(Score::Mate(-5).to_cp() < Score::Centipawns(-900).to_cp());
    }

    #[test]
    fn lines_without_a_score_are_ignored() {
        assert_eq!(parse_score("info depth 1 currmove e2e4", 1), None);
        assert_eq!(parse_score("bestmove e2e4 ponder e7e5", 1), None);
        assert_eq!(parse_score("info string Skill Level set", 1), None);
    }

    #[test]
    fn side_to_move_is_read_from_the_fen() {
        assert!(white_to_move(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        ));
        assert!(!white_to_move(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1"
        ));
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use std::time::{Duration, Instant};

    const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    fn fake_engine() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_engine.py"))
    }

    fn recv(link: &EngineLink) -> EngineReply {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(reply) = link.try_recv() {
                return reply;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the engine"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn handshake_search_and_bestmove_round_trip() {
        let mut uci = Uci::spawn(&fake_engine()).expect("fake engine should start");
        uci.is_ready().unwrap();

        let (score, best) = uci.search(START_FEN, "depth 8").unwrap();
        // The deepest info line wins, and White to move means no sign flip.
        assert_eq!(score, Score::Centipawns(34));
        assert_eq!(best, "e2e4");

        uci.quit();
    }

    #[test]
    fn black_to_move_scores_are_reported_from_whites_side() {
        let mut uci = Uci::spawn(&fake_engine()).unwrap();
        let black_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1";

        let (score, _) = uci.search(black_fen, "depth 8").unwrap();
        assert_eq!(score, Score::Centipawns(-34));

        uci.quit();
    }

    #[test]
    fn worker_thread_serves_analysis_and_opponent_moves() {
        unsafe { std::env::set_var("STOCKFISH_PATH", fake_engine()) };

        let link = start(5);
        assert!(link.available, "engine should be reported available");

        link.send(EngineRequest::Analyse {
            fen: START_FEN.to_string(),
            ply: 0,
        });
        match recv(&link) {
            EngineReply::Analysis {
                ply,
                score,
                best_move,
            } => {
                assert_eq!(ply, 0);
                assert_eq!(score, Score::Centipawns(34));
                assert_eq!(best_move, "e2e4");
            }
            _ => panic!("expected an analysis reply"),
        }

        link.send(EngineRequest::SetSkill(12));
        link.send(EngineRequest::PlayMove {
            fen: START_FEN.to_string(),
            ply: 1,
        });
        match recv(&link) {
            EngineReply::OpponentMove { ply, uci } => {
                assert_eq!(ply, 1);
                assert_eq!(uci, "e2e4");
            }
            _ => panic!("expected an opponent move"),
        }

        unsafe { std::env::remove_var("STOCKFISH_PATH") };
    }

    #[test]
    fn a_missing_binary_is_reported_rather_than_panicking() {
        let link = start_with(None, 5);
        assert!(!link.available);
        assert!(
            link.status.contains("not found"),
            "status was: {}",
            link.status
        );
    }

    #[test]
    fn an_explicit_path_to_a_non_file_is_not_accepted() {
        unsafe { std::env::set_var("STOCKFISH_PATH", "/nonexistent/stockfish") };
        let found = discover_stockfish();
        unsafe { std::env::remove_var("STOCKFISH_PATH") };

        // It may still fall back to a real install, but never to the bad path.
        assert_ne!(found, Some(PathBuf::from("/nonexistent/stockfish")));
    }
}
