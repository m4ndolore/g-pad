# Learn mode: kid-first feedback

## The problem

The tutor's replies are sentences — "Yes! Three and four make seven. What
makes eight?" — written in the reply hand across the feedback strip. A
four-to-eight-year-old cannot read that. The session is supervised, so an
adult CAN read a hint aloud, but the child should get the emotional payload
— right, almost, try again — with zero reading help.

## The design

**Two audiences, two registers.** Every practice verdict now carries:

1. **For the child: symbols and one-to-three capital words.** A big check
   and stars for YES; the cheer printed HUGE and centered ("GREAT JOB!").
   For ALMOST/NO the underline under the answer plus "SO CLOSE!" /
   "TRY AGAIN!". Printed instantly in the UI font — no handwriting
   animation to sit through, feedback lands like a teacher's stamp.
2. **For the grown-up: one small hint line.** Only after ALMOST/NO, one
   short sentence in smaller print beneath the cheer, meant to be read
   aloud. Never the answer.

**Protocol** (the tutor instruction demands, the parser enforces):

```
YES GREAT JOB
ALMOST SO CLOSE
Your 3 is facing the wrong way.
```

Line 1: verdict word + one-to-three capital cheer words a young child can
read. Line 2 (ALMOST/NO only): the grown-up hint. No follow-up questions,
no other prose.

**Client-side enforcement** — the model can ramble; the sheet cannot. The
parser takes the rest of the verdict line as the cheer only when it is at
most three words; anything longer, and any further lines, is demoted to
the hint. An empty cheer falls back to a fixed word per verdict (GREAT
JOB! / SO CLOSE! / TRY AGAIN!), so the child's channel is always short,
always uppercase, always legible.

**Games tightened** the same direction: critter captions at most eight
very simple words, guesses at most fifteen; the story keeps its
three-short-sentence beats (the adult narrates those by design).

## What does not change

The verdict words and their consequences (ladder, treat counter, auto-deal
dwell), the anchored-mark vocabulary, the check beside the answer, the
clean-retry repaint, and the Unknown rule: an unreadable verdict moves
nothing.
