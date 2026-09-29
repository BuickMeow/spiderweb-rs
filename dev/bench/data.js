window.BENCHMARK_DATA = {
  "lastUpdate": 1790661698007,
  "repoUrl": "https://github.com/BuickMeow/spiderweb-rs",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "email": "3347830431@qq.com",
            "name": "节能降耗",
            "username": "BuickMeow"
          },
          "committer": {
            "email": "3347830431@qq.com",
            "name": "节能降耗",
            "username": "BuickMeow"
          },
          "distinct": true,
          "id": "7ddd7c1eee89ca8cd05787d0e80184237392a4b2",
          "message": "ci: init gh-pages with plumbing instead of a worktree\n\n- the worktree kept gh-pages checked out, so the benchmark action's\n  fetch into refs/heads/gh-pages was refused by git\n- build the orphan commit with hash-object/mktree/commit-tree and push it\n  directly; nothing stays checked out",
          "timestamp": "2026-09-29T13:54:19+08:00",
          "tree_id": "b914e256b7b1a05464ba634209852777f762e3ef",
          "url": "https://github.com/BuickMeow/spiderweb-rs/commit/7ddd7c1eee89ca8cd05787d0e80184237392a4b2"
        },
        "date": 1790661697004,
        "tool": "cargo",
        "benches": [
          {
            "name": "line_notes_64keys",
            "value": 5180,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "tumour_line_64keys",
            "value": 1274248,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "custom_spam_100k",
            "value": 4803215,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "funnel_spam_100k",
            "value": 10175910,
            "range": "± 0",
            "unit": "ns/iter"
          },
          {
            "name": "render_single_100k",
            "value": 11388067,
            "range": "± 0",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}