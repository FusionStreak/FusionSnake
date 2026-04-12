window.BENCHMARK_DATA = {
  "lastUpdate": 1776014650845,
  "repoUrl": "https://github.com/FusionStreak/FusionSnake",
  "entries": {
    "FusionSnake Benchmarks": [
      {
        "commit": {
          "author": {
            "name": "Sayfullah",
            "username": "FusionStreak",
            "email": "sayfullaheid@gmail.com"
          },
          "committer": {
            "name": "Sayfullah",
            "username": "FusionStreak",
            "email": "sayfullaheid@gmail.com"
          },
          "id": "12c05088ba9233522b53a4e6b92949e28c102790",
          "message": "fix: update benchmark output format to use bencher and adjust output file name",
          "timestamp": "2026-04-12T17:10:59Z",
          "url": "https://github.com/FusionStreak/FusionSnake/commit/12c05088ba9233522b53a4e6b92949e28c102790"
        },
        "date": 1776014650633,
        "tool": "cargo",
        "benches": [
          {
            "name": "evaluate/duel_7x7",
            "value": 1013,
            "range": "± 14",
            "unit": "ns/iter"
          },
          {
            "name": "evaluate/4snake_11x11",
            "value": 2338,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "evaluate/late_game_11x11",
            "value": 2192,
            "range": "± 74",
            "unit": "ns/iter"
          },
          {
            "name": "evaluate/terminal_win",
            "value": 3,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "search/duel_7x7_50ms",
            "value": 31358465,
            "range": "± 229873",
            "unit": "ns/iter"
          },
          {
            "name": "search/4snake_11x11_50ms",
            "value": 50918633,
            "range": "± 3070180",
            "unit": "ns/iter"
          },
          {
            "name": "search_budget/duel_7x7_150ms",
            "value": 122524517,
            "range": "± 1086926",
            "unit": "ns/iter"
          },
          {
            "name": "search/late_game_11x11_50ms",
            "value": 52531410,
            "range": "± 858583",
            "unit": "ns/iter"
          },
          {
            "name": "apply_moves/duel_7x7",
            "value": 171,
            "range": "± 6",
            "unit": "ns/iter"
          },
          {
            "name": "apply_moves/4snake_11x11",
            "value": 327,
            "range": "± 10",
            "unit": "ns/iter"
          },
          {
            "name": "safe_moves/snake_0",
            "value": 118,
            "range": "± 3",
            "unit": "ns/iter"
          },
          {
            "name": "safe_moves/snake_1",
            "value": 118,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "safe_moves/snake_2",
            "value": 117,
            "range": "± 2",
            "unit": "ns/iter"
          },
          {
            "name": "safe_moves/snake_3",
            "value": 133,
            "range": "± 9",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}