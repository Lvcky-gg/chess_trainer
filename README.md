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
| `STOCKFISH_PATH` | — | Explicit path to the engine binary |

## Controls

| | |
|---|---|
| Click a piece, then a destination | Move. Legal targets are highlighted, captures in red |
| `H` | Toggle a hint showing the analyst's preferred move |
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

## Layout

| File | Responsibility |
|---|---|
| `src/main.rs` | App wiring, engine request/reply loop, keyboard |
| `src/game.rs` | Rules (via shakmaty), move grading, takebacks |
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
