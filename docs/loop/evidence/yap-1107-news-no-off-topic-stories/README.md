# yap-1107-news-no-off-topic-stories — evidence

Coder B1, loop three, written without compiling (no-compile rule). The
integrator fills in the compiled lines marked INTEGRATOR.

## Acceptance 1 — 60 headlines vs the hand-labelled key

Fixture: `desktop/src-tauri/tests/fixtures/news_headlines_60.json` (60
headlines with outlet: the three World Wide Technology golf headlines first,
then tricky ones: "Technology stocks fall as bond yields climb", "World
Series: ...", "The science of sleep ...", "World Cup qualifier ...", "Apple
stock drops after earnings miss", "Culture war in Congress ...", "Tech
billionaire buys NBA team ...", "S&P 500 hits record high on rate-cut hopes",
"Dow jumps 400 points as inflation cools", "Shohei Ohtani homers twice ...").
Answer key: `news_headlines_60.labels.json` beside it (US politics is
labelled world; crypto prices stocks).

Coder pre-check (a Python port of the same tables and rules, run over the
fixture before commit; NOT the Rust test):

    agree 60 / 60, sports/entertainment in Tech: none

Integrator run, 2026-11-08T06:56:13Z, integration-2 at 4f62be40 (`cargo test --features custom-protocol --lib news:: -- --nocapture`, exit 0):

    news classify fixture agree=60/60 misses=[]
    test result: ok. 90 passed; 0 failed; 0 ignored; 0 measured; 1351 filtered out

## Acceptance 4 — headless capture

INTEGRATOR: `node scripts/news-why-shots.mjs --out docs/loop/evidence/yap-1107-news-no-off-topic-stories`
writes `news-why-1280x1400@2x.png` and exits 1 if any card lacks the
"From <outlet> (<category>) · matches <topic>" line or shows a sports or
entertainment outlet.

Integrator run, 2026-11-08T06:56:13Z, integration-2 at 4f62be40: exit 0, `ok cards=3`; every card
carries its "From <outlet> (<category>) · matches <topic>" line and no sports or
entertainment outlet appears. Capture: `news-why-1280x1400@2x.png`.

## Which file proves which line

1. sources.rs tests `sixty_headlines_match_the_answer_key_with_no_sport_in_tech`, `the_three_golf_headlines_are_sports_with_or_without_the_outlet`
2. sources.rs `declared_source_beats_keywords_and_cloud`, `cloud_label_beats_keywords_for_an_undeclared_outlet`, `keywords_are_whole_word_and_the_deny_list_vetoes_first`; cache.rs `one_cloud_call_per_batch_and_none_for_cached_urls`, `a_failed_cloud_call_falls_through_to_the_keywords`
3. sources.rs `off_topic_stories_are_dropped_never_relabelled` (Wilson's seven topics)
4. scripts/news-why-shots.mjs + NewsBriefing.tsx `data-news-why`
5. sources.rs `fetch_plan_lists_sports_only_when_sports_is_picked`; feeds.rs refresh uses `sources::fetch_plan`
6. rank.rs `topic_search_words_add_no_title_hit`, `topic_score_matches_interests_and_drops_off_topic`
