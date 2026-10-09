#!/usr/bin/env python3
"""Pick the chess trainer's puzzles from the Lichess puzzle database.

The database is public domain (CC0): https://database.lichess.org/#puzzles
Download lichess_db_puzzle.csv.zst, then:

    zstdcat lichess_db_puzzle.csv.zst | python3 scripts/chess-puzzles.py > assets/chess/puzzles.txt

The kids' checkmates need python-chess (`pip install chess`) to keep only
positions with a single mating move.

Output, one puzzle per line: category|rating|label|fen|moves
  category  tactic, endgame, or opening; mate is kids mode's checkmate in one
  rating    the Lichess puzzle rating
  label     the motif, the endgame type, or the opening family
  fen       the position before the opponent's setup move
  moves     UCI moves: the setup move, then the solution alternating sides

The pick is deterministic: the same database gives the same file.
"""
import csv
import hashlib
import sys

PER_CATEGORY = 600
BANDS = [(1400, 1700), (1700, 2000), (2000, 2300)]
MIN_POPULARITY = 85
MIN_PLAYS = 1000
MAX_DEVIATION = 80

# Kids mode: checkmate in one, easy and uncluttered. Fewest pieces first, so a
# six-year-old can read the board before the puzzle.
KIDS_MATES = 300
KIDS_BAND = (400, 1000)
KIDS_MIN_POPULARITY = 80
KIDS_MIN_PLAYS = 500
KIDS_MAX_DEVIATION = 90

# First match wins: the motif a player would name for the puzzle.
MOTIFS = [
    "smotheredMate", "backRankMate", "doubleCheck", "discoveredAttack", "fork",
    "pin", "skewer", "xRayAttack", "deflection", "attraction", "interference",
    "clearance", "intermezzo", "zugzwang", "trappedPiece", "quietMove",
    "sacrifice", "mateIn2", "mateIn3", "mateIn4", "hangingPiece", "defensiveMove",
]
ENDGAMES = [
    "queenRookEndgame", "rookEndgame", "queenEndgame", "bishopEndgame",
    "knightEndgame", "pawnEndgame",
]


def words(camel):
    out = ""
    for ch in camel:
        out += (" " + ch.lower()) if ch.isupper() else ch
    return out.replace("mate in", "mate in ").replace("  ", " ").strip()


def classify(themes, opening_tags):
    t = set(themes.split())
    if "opening" in t and opening_tags:
        family = opening_tags.split()[0].replace("_", " ")
        return "opening", family
    if "endgame" in t:
        kind = next((e for e in ENDGAMES if e in t), None) or next((m for m in MOTIFS if m in t), None)
        return "endgame", words(kind) if kind else "endgame"
    if "middlegame" in t:
        motif = next((m for m in MOTIFS if m in t), None)
        if motif:
            return "tactic", words(motif)
    return None, None


def pieces(fen):
    return sum(ch.isalpha() for ch in fen.split()[0])


def kids_mate(row, rating):
    """A sort key when the row is an easy, popular mate in one, else None."""
    if "mateIn1" not in row["Themes"].split() or not KIDS_BAND[0] <= rating < KIDS_BAND[1]:
        return None
    if (int(row["Popularity"]) < KIDS_MIN_POPULARITY or int(row["NbPlays"]) < KIDS_MIN_PLAYS
            or int(row["RatingDeviation"]) > KIDS_MAX_DEVIATION):
        return None
    return (pieces(row["FEN"]), hashlib.sha1(row["PuzzleId"].encode()).hexdigest())


def one_mate(line):
    """Whether the mate in one has exactly one mating move. The trainer
    accepts only the listed move, so a second mate would mark a child's right
    answer wrong."""
    import chess

    fen, moves = line.split("|")[3:5]
    board = chess.Board(fen)
    board.push_uci(moves.split()[0])
    mates = 0
    for move in board.legal_moves:
        board.push(move)
        mates += board.is_checkmate()
        board.pop()
    return mates == 1


def main():
    picked = {c: {b: [] for b in BANDS} for c in ("tactic", "endgame", "opening")}
    mates = []
    for row in csv.DictReader(sys.stdin):
        try:
            rating = int(row["Rating"])
            mate_key = kids_mate(row, rating)
            if mate_key:
                mates.append((mate_key, f'mate|{rating}|mate in 1|{row["FEN"]}|{row["Moves"]}'))
            if (int(row["Popularity"]) < MIN_POPULARITY or int(row["NbPlays"]) < MIN_PLAYS
                    or int(row["RatingDeviation"]) > MAX_DEVIATION):
                continue
        except ValueError:
            continue
        band = next((b for b in BANDS if b[0] <= rating < b[1]), None)
        if band is None:
            continue
        cat, label = classify(row["Themes"], row.get("OpeningTags", ""))
        if cat is None:
            continue
        key = hashlib.sha1(row["PuzzleId"].encode()).hexdigest()
        line = f'{cat}|{rating}|{label}|{row["FEN"]}|{row["Moves"]}'
        picked[cat][band].append((key, line))
    per_band = PER_CATEGORY // len(BANDS)
    for cat in ("tactic", "endgame", "opening"):
        for band in BANDS:
            for _, line in sorted(picked[cat][band])[:per_band]:
                print(line)
    kept = 0
    for _, line in sorted(mates):
        if kept == KIDS_MATES:
            break
        if one_mate(line):
            print(line)
            kept += 1


if __name__ == "__main__":
    main()
