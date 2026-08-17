# Chess Trainer

A 3D chess trainer built with Bevy. You play Stockfish on a 3D board, and a
*second*, full-strength Stockfish grades every move you make.

The two engines are deliberately separate. The opponent is skill-limited to
whatever level you pick; the analyst always runs at full strength. If one
weakened engine did both jobs, the coaching would be as weak as the opponent.

## Requirements

Stockfish is not bundled. On Arch:

```bash
yay -S stockfish          # AUR
```

The app finds the binary via `STOCKFISH_PATH`, then `PATH`, then the usual
install locations. **Without it the board still runs**, falling back to moving
both sides by hand so nothing is blocked on the install.

## Running

```bash
cargo run --release
```

Release is worth it: the six piece models are ~25 MB / 500k triangles each and
are parsed at startup.

| Variable | Default | Meaning |
|---|---|---|
| `CHESS_SKILL` | `5` | Stockfish skill level, `0`–`20` |
| `CHESS_SIDE` | `white` | `black` to play the other side |
| `CHESS_DRILL` | — | Opening name or ECO code to drill, e.g. `Najdorf`, `B90` |
| `STOCKFISH_PATH` | — | Explicit path to the engine binary |

## Controls

| | |
|---|---|
| Click a piece, then a destination | Move. Legal targets are highlighted, captures in red |
| `H` | Toggle a hint showing the analyst's preferred move |
| `S` | Toggle the accuracy breakdown (opens by itself at game over) |
| `U` | Take back your last move (and the engine's reply) |
| `N` | New game |
| `[` / `]` | Lower / raise engine strength |
| Right-drag, scroll, arrow keys | Orbit and zoom the camera |

## Coaching

After each of your moves the analyst re-evaluates the position. The difference
between the evaluation before and after your move is what the move gave away,
and it is classified the way most online trainers do it:

| Centipawns lost | Verdict |
|---|---|
| ≤ 10 | Best move |
| 11–40 | Good move |
| 41–90 | Inaccuracy |
| 91–200 | Mistake |
| > 200 | Blunder |

When your move was not the analyst's choice, the better move is named in SAN.
The bar on the left is the running evaluation, always from White's point of view.

### Openings

Centipawn grading is the wrong instrument in the opening. A main line can
"lose" 20cp to a depth-14 search and still be the move everyone has played for
a century, so the trainer names theory instead of scoring it:

- while the game is in book, the panel shows the line — `Sicilian Defense:
  Najdorf Variation (B90)` — narrowing as the game gets more specific;
- moves in book are graded **Theory**, not Best/Inaccuracy, and are excluded
  from accuracy entirely, so a memorised opening cannot flatter your numbers
  and a main line cannot be called a mistake;
- when you leave theory, it says at which move, and which moves would have
  stayed in it.

`src/assets/openings.tsv` is the [Lichess chess-openings
database](https://github.com/lichess-org/chess-openings) (3,810 named lines,
CC0 public domain). It is embedded at compile time and replayed into a position
set at startup — 7,855 positions in about 10ms.

**Named is not the same as usual.** The database names some very obscure moves:
`3...a5` in the Ruy Lopez is the *Bulgarian Variation*, so "in book" alone would
call it as respectable as `3...a6`. The book therefore also records how many
database lines run through each position, and the panel reports it — `on 84 book
lines` versus `on 1 book line`. It is a rough stand-in for popularity, which
this dataset does not carry; the honest version would need the Lichess opening
explorer's game counts.

### Drilling an opening

```bash
CHESS_DRILL="Najdorf" CHESS_SIDE=black cargo run --release
```

The trainer then plays that repertoire and asks *you* for the moves. Naming
theory as it goes by teaches recognition; a drill is the other direction — the
position is given and the move is the answer.

- the opponent replies from the same repertoire, so the line stays on rails;
- a wrong move is taken back and the question asked again, naming the book move;
- a retry cannot earn the point, so the score is `7/9 first try`;
- at the end of the line, the moves you missed are listed.

`CHESS_DRILL` matches an opening name (case-insensitive substring) or an exact
ECO code. A filter matching nothing is reported and the app plays normally
rather than silently drilling the whole database.

A drill needs no engine — the book poses the questions and plays the replies —
so it works with Stockfish absent. Takeback (`U`) is refused while drilling,
since the drill owns the board and does its own retries. The hint key still
shows the *analyst's* move, which is not necessarily the book move.

Positions are keyed by **Polyglot-compatible Zobrist hash**, not by move
sequence, so a line reached by transposition is recognised as the same opening —
which is most of the point of naming openings at all. shakmaty produces those
hashes directly (its `zobrist` module is tested against the Polyglot reference
values), so no opening-book crate is involved.

### Accuracy by stage

Per-move grades say a move cost 0.6 pawns; they never say *which part of your
game* keeps leaking. Every graded move is therefore also filed under the stage
it was chosen in, and `S` shows the breakdown:

```
Opening      94%   3 moves  (+6 book)
Middlegame   71%   18 moves
   2 mistakes, 1 blunder
   worst  19. Qd2  -3.10
Endgame      88%   9 moves

Overall      82%
Weakest      Middlegame
```

The move counts are of *scored* moves; theory is listed separately and never
averaged in. A stage played entirely from book reads `all theory` rather than
a percentage, because there is nothing of yours in it to score.

The stage is that of the position the move was *chosen in*, so the blunder that
trades the last queens is charged to the middlegame rather than to the endgame
it created. Stages are classified from the position alone:

| Stage | Test |
|---|---|
| Endgame | Non-pawn material (Q9 R5 B3 N3, both sides) ≤ 20 |
| Opening | Not an endgame, move ≤ 15, **and** ≥ 4 minor pieces still on home squares |
| Middlegame | Everything else |

Both opening bounds are needed: a gambit can leave a piece at home until move
30, and a quiet exchange line can be fully developed by move 8.

Accuracy is not linear in centipawns. It is Lichess' formula — centipawns are
first mapped to winning chances, then the drop across your move is mapped to
0–100 — because giving away 150cp from a level position costs far more of the
game than giving away 150cp when already three pawns up.

Takebacks discard the reviews of the moves they undo, so a position you rewound
is not still counted against you.

## Layout

| File | Responsibility |
|---|---|
| `src/main.rs` | App wiring, engine request/reply loop, keyboard |
| `src/drill.rs` | Opening drill: poses book moves, marks answers, retries |
| `src/game.rs` | Rules (via shakmaty), move grading, takebacks |
| `src/openings.rs` | Named opening theory, book membership, continuations |
| `src/review.rs` | Stage classification, accuracy, per-stage summary |
| `src/engine.rs` | UCI client, engine worker thread, score normalisation |
| `src/scene.rs` | Board, piece models, highlighting, camera, clicking |
| `src/stl.rs` | Binary STL loading, decimation, normalisation |
| `src/ui.rs` | Status, feedback, move list, evaluation bar |

### Notes on two decisions

**UCI is spoken directly** rather than through the `stockfish` crate. That crate
fetches the FEN by interleaving a `d` command with the running search, which can
consume the `bestmove` line and leave the read loop blocked forever — most
likely in exactly the timed-search call an opponent needs. It also has no way to
bound thinking time. Talking UCI directly is about 80 lines and removes both
problems.

Note that UCI reports scores relative to the side to move. Everything above the
`Uci::search` boundary is normalised to White, so consecutive evaluations can be
subtracted directly.

**The STL models are decimated at load.** At ~500k triangles each, a full board
would be ~16M triangles per frame before shadows. Vertex-clustering decimation
cuts each to 45–145k, and the six meshes are shared across all 32 pieces. The
models are also authored Z-up in arbitrary units, so they are rotated to Bevy's
Y-up, centred, and scaled to a set height with the base resting on `y=0`.

## Tests

```bash
cargo test --release
```

The engine tests run against `tests/fake_engine.py`, a stand-in UCI engine, so
the protocol path is covered even where Stockfish is not installed.
