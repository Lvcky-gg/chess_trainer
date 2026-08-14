#!/usr/bin/env python3
"""A minimal UCI engine used to test the engine plumbing without Stockfish.

It speaks just enough of the protocol to exercise the real code path: the
handshake, options, and searches that emit info lines before a bestmove. The
move it returns is always legal in the standard starting position so the game
layer can accept it.
"""

import sys


def reply(line: str) -> None:
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def main() -> None:
    skill = None
    # Real engines print a banner before anything is asked of them.
    reply("Fake Engine 1.0 by the test suite")

    for raw in sys.stdin:
        cmd = raw.strip()

        if cmd == "uci":
            reply("id name FakeEngine")
            reply("id author test")
            reply("option name Skill Level type spin default 20 min 0 max 20")
            reply("uciok")

        elif cmd == "isready":
            reply("readyok")

        elif cmd.startswith("setoption"):
            if "Skill Level" in cmd:
                skill = cmd.rsplit(" ", 1)[-1]
                # Info strings must not be mistaken for a score line.
                reply(f"info string Skill Level set to {skill}")

        elif cmd == "ucinewgame":
            pass

        elif cmd.startswith("position"):
            pass

        elif cmd.startswith("go"):
            # Noise the parser has to skip, then deepening scores. The last one
            # before bestmove is the score the caller should end up with.
            reply("info depth 1 currmove e2e4 currmovenumber 1")
            reply("info depth 1 score cp 12 pv e2e4")
            reply("info depth 8 score cp 34 pv e2e4 e7e5")
            reply("bestmove e2e4 ponder e7e5")

        elif cmd == "quit":
            return


if __name__ == "__main__":
    main()
