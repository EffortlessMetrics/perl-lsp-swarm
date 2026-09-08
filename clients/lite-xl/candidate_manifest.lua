-- Checked candidate composition manifest (#11170).
--
-- This module is the single authority binding exact reviewed patch leaves
-- to named claim profiles for the Lite XL integration train:
--
--   exact upstream base (pinned pristine blobs)
--   + exact merged internal candidates (one reviewed leaf each)
--   -> one installable exact-source client tree per profile
--
-- Laws this data must keep (enforced by tests/compose_manifest_test.lua
-- against real history, and by compose.lua at materialization time):
--   - every component is ONE merged internal candidate (squash commit) in
--     this repository; its recorded per-path content digests are the git
--     blob SHAs of the staged files at that commit;
--   - hard prerequisites are exactly the staged-lineage chain edges
--     recomputed from history: on every changed path, a leaf requires its
--     immediate predecessor leaf. Composition is therefore prefix-closed
--     per path and the last declared dependent owns the composed bytes;
--   - only merged/current candidates may enter a profile; draft or stale
--     SHAs fail closed (compose.lua typed stale_candidate_sha);
--   - profiles declare explicit membership and an admitted class
--     envelope; they share components but never inherit evidence by
--     naming;
--   - upstream states use the #10739 vocabulary; everything here is
--     internal-only. Composed profiles never create submitted/accepted/
--     released upstream claims (#10739 keeps owning one-leaf packets);
--   - application order is DERIVED from these dependencies at
--     composition time and never declared lexically.
--
-- The pristine base copies under leaves/base/ hash to the documented
-- upstream blob digests below (verified byte-exactly against
-- lite-xl/lite-xl-lsp @ d1432ae0736cd9531798b4bc1221835f534cc689).

return {
  schema = "candidate-manifest.v1",

  meta = {
    base_dir = "leaves/base",
    digest_algorithm = "git-blob-sha1",
    tree_layout = "upstream-root",
    composition_law = "whole-file snapshots from merged internal candidates "
      .. "applied over the pinned pristine base in dependency-derived "
      .. "topological order; prefix closure enforced per path; the last "
      .. "declared dependent owns each composed path",
  },

  upstream_base = {
    repository = "lite-xl/lite-xl-lsp",
    ref = "d1432ae0736cd9531798b4bc1221835f534cc689",
    files = {
      ["diagnostics.lua"] = "c06bec4955d7fbfd8f3a2753fba26c04247b09e0",
      ["helpdoc.lua"] = "42d7a07f23fa9f254e28ba2ab2c858aded3122d5",
      ["init.lua"] = "7b38c3a97c68877d2391753adb09e49ec57397d3",
      ["json.lua"] = "eb36b8fa947ff1189b02ce03d257b80a86fdac64",
      ["listbox.lua"] = "33284b02995781d897add3b44c4d66aac64d299e",
      ["server.lua"] = "33c8ccae7362ddb01aa980bff024a4ef1682c8f9",
      ["symbolresults.lua"] = "96c39cd5ee1b765c85c6f7dc5eb1cb90386994ad",
      ["timer.lua"] = "c25fefa44e65d1f3a8c52e555080a61195ececae",
      ["util.lua"] = "588c101aa97ef0d112926aac316e7a95a52a6994",
    },
  },

  -- Combined-tree proof selection: which focused #11103 suite proves which
  -- staged module, with the generated-copy argument order each suite's
  -- header documents. The runner substitutes generated tree paths.
  proof_matrix = {
    ["capability_manifest.lua"] = {
      { suite = "capability_manifest_test.lua",
        modules = { "capability_manifest.lua" } },
    },
    ["diagnostics.lua"] = {
      { suite = "diagnostics_currentness_test.lua",
        modules = { "init.lua", "diagnostics.lua" } },
      { suite = "diagnostic_position_test.lua",
        modules = { "init.lua", "diagnostics.lua" } },
    },
    ["init.lua"] = {
      { suite = "init_document_session_test.lua",
        modules = { "init.lua" } },
      { suite = "init_request_currentness_test.lua",
        modules = { "init.lua" } },
      { suite = "init_configuration_items_test.lua",
        modules = { "init.lua", "util.lua" } },
      { suite = "init_show_document_outcome_test.lua",
        modules = { "init.lua" } },
      { suite = "init_completion_resolve_test.lua",
        modules = { "init.lua" } },
      { suite = "init_command_projection_test.lua",
        modules = { "init.lua", "capability_manifest.lua" } },
    },
    ["json.lua"] = {
      { suite = "json_decode_test.lua", modules = { "json.lua" } },
    },
    ["server.lua"] = {
      { suite = "server_frame_test.lua",
        modules = { "server.lua", "json.lua" } },
      { suite = "server_logging_test.lua",
        modules = { "server.lua", "util.lua" } },
      { suite = "server_message_scheduling_test.lua",
        modules = { "server.lua" } },
      { suite = "server_initialize_capabilities_test.lua",
        modules = { "server.lua", "capability_manifest.lua" } },
    },
    ["util.lua"] = {
      { suite = "util_show_document_test.lua", modules = { "util.lua" } },
      { suite = "util_config_merge_test.lua", modules = { "util.lua" } },
      { suite = "util_file_uri_test.lua", modules = { "util.lua" } },
    },
  },

  components = {
    {
      id = "leaf_11197", issue = 11197, pull_request = 11963,
      title = "malformed JSON terminates with one typed decode error",
      candidate_sha = "612a001208177d7972d808c15e1c6bd8be86a854",
      changed_paths = { "json.lua" },
      hard_prerequisites = {},
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 11197,
      conflict_keys = { "lite-xl.upstream.json.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["json.lua"] = "1125e0cd55d82b4b49001fe24cde80856301249a" },
    },
    {
      id = "leaf_11136", issue = 11136, pull_request = 11980,
      title = "JSON null and empty array/object identity preserved",
      candidate_sha = "894662ac9412658ae06468817d83e6be6077701b",
      changed_paths = { "json.lua" },
      hard_prerequisites = { "leaf_11197" },
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 11136,
      conflict_keys = { "lite-xl.upstream.json.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["json.lua"] = "6a76a2c6c9de1c2240dd6c151e609d13bd0b7363" },
    },
    {
      id = "leaf_11183", issue = 11183, pull_request = 11984,
      title = "JSON numeric and string identity preserved for request IDs",
      candidate_sha = "80139c55d9fd03cb9d17057938258dfa7a68fd81",
      changed_paths = { "json.lua" },
      hard_prerequisites = { "leaf_11136" },
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 11183,
      conflict_keys = { "lite-xl.upstream.json.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["json.lua"] = "871c5c6a3b0b4116471f6623ac40f612df32f5df" },
    },
    {
      id = "leaf_11194", issue = 11194, pull_request = 11993,
      title = "JSON UTF-8 and Unicode scalar validation",
      candidate_sha = "2e791bb8dcaae3201d4cd19b8e7575fd1db66563",
      changed_paths = { "json.lua" },
      hard_prerequisites = { "leaf_11183" },
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 11194,
      conflict_keys = { "lite-xl.upstream.json.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["json.lua"] = "7516599d52870ce476076fb7c7952b20b104a62c" },
    },
    {
      id = "leaf_11186", issue = 11186, pull_request = 11996,
      title = "bounded JSON nesting and structural decode/encode work",
      candidate_sha = "3974208cbfed08e43556bd7d1eabdae402fc4f63",
      changed_paths = { "json.lua" },
      hard_prerequisites = { "leaf_11194" },
      class = "security",
      upstream_state = "internal",
      owner_issue = 11186,
      conflict_keys = { "lite-xl.upstream.json.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["json.lua"] = "72ac0aea806e2a1f2d557fece904a8a590f58bfb" },
    },
    {
      id = "leaf_11151", issue = 11151, pull_request = 12003,
      title = "validated bounded inbound Content-Length frames",
      candidate_sha = "974274e2e8ff86a360b1ad8c1ad2d46da0b76e20",
      changed_paths = { "server.lua" },
      hard_prerequisites = {},
      class = "security",
      upstream_state = "internal",
      owner_issue = 11151,
      conflict_keys = { "lite-xl.upstream.server.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["server.lua"] = "860926d889db10a84f90763bf99a3c9fa21ca9b1" },
    },
    {
      id = "leaf_11155", issue = 11155, pull_request = 12015,
      title = "bounded redacted protocol failure logs",
      candidate_sha = "31a72463c324172099494529205a0067a4d5942e",
      changed_paths = { "server.lua", "util.lua" },
      hard_prerequisites = { "leaf_11151" },
      class = "security",
      upstream_state = "internal",
      owner_issue = 11155,
      conflict_keys = { "lite-xl.upstream.server.lua",
        "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["server.lua"] = "06115a03993fd1e48f36105f38ce8fc7bd1a8074",
        ["util.lua"] = "5586c714a12ac20907e0188909b4357b7241de3a",
      },
    },
    {
      id = "leaf_11162", issue = 11162, pull_request = 12025,
      title = "classified showDocument URIs with shell-free launch",
      candidate_sha = "61a57f2e405ad1e8952d180ede851cf2148e79c5",
      changed_paths = { "util.lua" },
      hard_prerequisites = { "leaf_11155" },
      class = "security",
      upstream_state = "internal",
      owner_issue = 11162,
      conflict_keys = { "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["util.lua"] = "b0e9164d8ae95fa87a4a9369db389e46666ee307" },
    },
    {
      id = "leaf_11115", issue = 11115, pull_request = 12029,
      title = "monotonic document version per server and document session",
      candidate_sha = "87fcc2d179918f9be79876e564fcb5d7f2fdc698",
      changed_paths = { "init.lua" },
      hard_prerequisites = {},
      class = "session",
      upstream_state = "internal",
      owner_issue = 11115,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["init.lua"] = "d8de89b6b760322ce7080362f1b9b782c37116f8" },
    },
    {
      id = "leaf_11108", issue = 11108, pull_request = 12036,
      title = "generation-bound request result admission",
      candidate_sha = "4e359b5a6f1e673c36301b1a6a8237fd4f038559",
      changed_paths = { "init.lua" },
      hard_prerequisites = { "leaf_11115" },
      class = "session",
      upstream_state = "internal",
      owner_issue = 11108,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["init.lua"] = "06db36111c8eb87d3301eec53436959deee1b3b7" },
    },
    {
      id = "leaf_11124", issue = 11124, pull_request = 12044,
      title = "provider-generation-bound push diagnostics publication",
      candidate_sha = "112bc2cb2a9cfacede83adb96f625c497374e80e",
      changed_paths = { "diagnostics.lua", "init.lua" },
      hard_prerequisites = { "leaf_11108" },
      class = "diagnostic",
      upstream_state = "internal",
      owner_issue = 11124,
      conflict_keys = { "lite-xl.upstream.diagnostics.lua",
        "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["diagnostics.lua"] = "4f3ac742adf6f917f39ff4dfe9b058cf77384040",
        ["init.lua"] = "3cb2161facdd7f5a315a5bc3f34b990cd29e5dc4",
      },
    },
    {
      id = "leaf_11147", issue = 11147, pull_request = 12061,
      title = "positional workspace/configuration answers",
      candidate_sha = "cb54fbd076a9504e6275daf90df071bd154c142f",
      changed_paths = { "init.lua" },
      hard_prerequisites = { "leaf_11124" },
      class = "configuration",
      upstream_state = "internal",
      owner_issue = 11147,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["init.lua"] = "87b203302429fc20241a4cb61e309eb7e3cbfca3" },
    },
    {
      id = "leaf_11128", issue = 11128, pull_request = 12047,
      title = "live-document diagnostic range projection",
      candidate_sha = "db036ad06ca73854e519bbdad46372d323d7367d",
      changed_paths = { "diagnostics.lua", "init.lua" },
      hard_prerequisites = { "leaf_11124", "leaf_11147" },
      class = "diagnostic",
      upstream_state = "internal",
      owner_issue = 11128,
      conflict_keys = { "lite-xl.upstream.diagnostics.lua",
        "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["diagnostics.lua"] = "b8c75663bd3fd70931d9b316fafcd98f59ea9aba",
        ["init.lua"] = "7794d905686e2134a8bd66f28d7294ab46981e47",
      },
    },
    {
      id = "leaf_11143", issue = 11143, pull_request = 12215,
      title = "typed workspace configuration merge",
      candidate_sha = "d27352952eb570a01eaff070edc4683967b241b5",
      changed_paths = { "init.lua", "util.lua" },
      hard_prerequisites = { "leaf_11162", "leaf_11128" },
      class = "configuration",
      upstream_state = "internal",
      owner_issue = 11143,
      conflict_keys = { "lite-xl.upstream.init.lua",
        "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "7136060a74188565fe036099adef854d408f1d84",
        ["util.lua"] = "a7cd8ef9fd93e1452e941165198b671ee9cb221d",
      },
    },
    {
      id = "leaf_11165", issue = 11165, pull_request = 12130,
      title = "single local file URI and path conversion authority",
      candidate_sha = "805a43efb05e8489957c447208814dc64a99face",
      changed_paths = { "diagnostics.lua", "init.lua", "server.lua",
        "util.lua" },
      hard_prerequisites = { "leaf_11155", "leaf_11143", "leaf_11128" },
      class = "document",
      upstream_state = "internal",
      owner_issue = 11165,
      conflict_keys = { "lite-xl.upstream.diagnostics.lua",
        "lite-xl.upstream.init.lua", "lite-xl.upstream.server.lua",
        "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["diagnostics.lua"] = "4d50d33536640f5b7d6768cfd35ec10d9ef4e8af",
        ["init.lua"] = "2ea51ad87a9b099089729390b75bc925b2444e1c",
        ["server.lua"] = "f2f06f952f0f1eaf67c80fad39a91ea3808adcbd",
        ["util.lua"] = "ad1d3334ffcc174e3f9de63120a015c8a2cb2972",
      },
    },
    {
      id = "leaf_10845", issue = 10845, pull_request = 12468,
      title = "presence-aware workspace configuration values",
      candidate_sha = "8117baca208ef3971d2eaf0820b33f59713d2c92",
      changed_paths = { "init.lua", "util.lua" },
      hard_prerequisites = { "leaf_11165" },
      class = "configuration",
      upstream_state = "internal",
      owner_issue = 10845,
      conflict_keys = { "lite-xl.upstream.init.lua",
        "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "c0f7d22d68bc6d828159bc3154d3f0b0b5e41728",
        ["util.lua"] = "d7bd86b00c9b469a2ac019cdfafc0e99e5dde3da",
      },
    },
    {
      id = "leaf_10873", issue = 10873, pull_request = 12513,
      title = "truthful window/showDocument outcomes",
      candidate_sha = "8227ed36107f087e39ffc6c2ad996823324ebbc0",
      changed_paths = { "init.lua", "util.lua" },
      hard_prerequisites = { "leaf_10845" },
      class = "provider",
      upstream_state = "internal",
      owner_issue = 10873,
      conflict_keys = { "lite-xl.upstream.init.lua",
        "lite-xl.upstream.util.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "eef3d1d0c742ea6acb6b585ebdc481d7c87483f1",
        ["util.lua"] = "49faf7e6e39c8365ff6b885419a66720fbf596ce",
      },
    },
    {
      id = "leaf_10833", issue = 10833, pull_request = 12544,
      title = "bounded queueing, coalescing, and explicit backpressure",
      candidate_sha = "b6e5b4cc47aeee572e9e3a2ff9fb0e68f0c410e9",
      changed_paths = { "init.lua", "server.lua" },
      hard_prerequisites = { "leaf_11165", "leaf_10873" },
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 10833,
      conflict_keys = { "lite-xl.upstream.init.lua",
        "lite-xl.upstream.server.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "60b39b8a352bbf96ad012c964a87d151a2afab92",
        ["server.lua"] = "d2e29c01b9ea328f6954079a5642bd1fb9ea9ba0",
      },
    },
    {
      id = "leaf_11188", issue = 11188, pull_request = 12547,
      title = "exact completion resolve pre-apply operation",
      candidate_sha = "652e89e736238bd64ee3941eee6b7d72b5afc671",
      changed_paths = { "init.lua" },
      hard_prerequisites = { "leaf_10833" },
      class = "provider",
      upstream_state = "internal",
      owner_issue = 11188,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = { ["init.lua"] = "36069141cf70bf3dfe909d3c4d7209474a1990f9" },
    },
    {
      id = "leaf_11172", issue = 11172, pull_request = 12599,
      title = "capability advertisement projection authority",
      candidate_sha = "1ebe95c8feecfb339a7038235789cf90d27b2c95",
      changed_paths = { "capability_manifest.lua", "init.lua", "server.lua" },
      hard_prerequisites = { "leaf_10833", "leaf_11188" },
      class = "advertisement",
      upstream_state = "internal",
      owner_issue = 11172,
      conflict_keys = { "lite-xl.upstream.capability_manifest.lua",
        "lite-xl.upstream.init.lua", "lite-xl.upstream.server.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["capability_manifest.lua"] =
          "f6c26f9aec94f34d459d3afa297047c67adcebce",
        ["init.lua"] = "24fb933a73f2b5f2f17f0fe01a4f6f8242fdf5bd",
        ["server.lua"] = "53adee31734e1b6a4c76662640d66bd792864400",
      },
    },
    {
      id = "leaf_10653", issue = 10653, pull_request = 12709,
      title = "workspace configuration never executes project-local Lua",
      candidate_sha = "7eea107b67d56944e883103f4624ceb32198824d",
      changed_paths = { "init.lua" },
      hard_prerequisites = { "leaf_11172" },
      class = "security",
      upstream_state = "internal",
      owner_issue = 10653,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "07966add53faa9b91c5fbeb87cc1a971dd26604b",
      },
    },
    {
      id = "leaf_10657", issue = 10657, pull_request = 12715,
      title = "single-send request ids with terminal typed timeouts",
      candidate_sha = "bb9ae34060c61043925ea47fb9de417cfdcccb28",
      changed_paths = { "init.lua", "server.lua" },
      hard_prerequisites = { "leaf_10653", "leaf_11172" },
      class = "protocol",
      upstream_state = "internal",
      owner_issue = 10657,
      conflict_keys = {
        "lite-xl.upstream.init.lua", "lite-xl.upstream.server.lua",
      },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "6c100e6c65e5817435e5f0c6e5ff84aa370e5bba",
        ["server.lua"] = "aa42bd58fd8d3b48bd115f662c3d2af2b9aeddd8",
      },
    },
    {
      id = "leaf_11198", issue = 11198, pull_request = 12670,
      title = "document symbols keep duplicate identities and exact "
        .. "navigation targets",
      candidate_sha = "722cfc77530cc3e0a9cc4abf2740d53696a121ca",
      changed_paths = { "init.lua" },
      hard_prerequisites = { "leaf_10657" },
      class = "document",
      upstream_state = "internal",
      owner_issue = 11198,
      conflict_keys = { "lite-xl.upstream.init.lua" },
      invalidation_inputs = { "upstream_base_ref", "candidate_sha" },
      content = {
        ["init.lua"] = "c5184e2657826ee6f9d7320bff4e70b6bb8119a9",
      },
    },
  },

  profiles = {
    {
      id = "lite_xl_protocol_baseline",
      claim = "Pristine exact-source anchor: unpatched upstream client "
        .. "modules for registration/profile compatibility evidence until "
        .. "a registration-class leaf lands.",
      admitted_classes = { advertisement = true, configuration = true },
      members = {},
    },
    {
      id = "lite_xl_exact_source_core",
      claim = "Security, protocol codec/framing/backpressure, document "
        .. "session and version currentness, provider request handling, "
        .. "diagnostics, configuration lifecycle, and advertisement "
        .. "authority over the exact upstream source.",
      admitted_classes = {
        security = true, protocol = true, session = true,
        document = true, diagnostic = true, provider = true,
        configuration = true, advertisement = true,
      },
      members = {
        "leaf_11197", "leaf_11136", "leaf_11183", "leaf_11194",
        "leaf_11186", "leaf_11151", "leaf_11155", "leaf_11162",
        "leaf_11115", "leaf_11108", "leaf_11124", "leaf_11147",
        "leaf_11128", "leaf_11143", "leaf_11165", "leaf_10845",
        "leaf_10873", "leaf_10833", "leaf_11188", "leaf_11172",
        "leaf_10653", "leaf_10657", "leaf_11198",
      },
    },
    {
      id = "lite_xl_workspace_fresh",
      claim = "The core envelope plus the workspace/file-family "
        .. "currentness surface reserved for #10691-selected leaves; "
        .. "membership is declared explicitly here and no evidence is "
        .. "inherited from any other profile by naming.",
      admitted_classes = {
        security = true, protocol = true, session = true,
        document = true, diagnostic = true, provider = true,
        configuration = true, advertisement = true,
      },
      members = {
        "leaf_11197", "leaf_11136", "leaf_11183", "leaf_11194",
        "leaf_11186", "leaf_11151", "leaf_11155", "leaf_11162",
        "leaf_11115", "leaf_11108", "leaf_11124", "leaf_11147",
        "leaf_11128", "leaf_11143", "leaf_11165", "leaf_10845",
        "leaf_10873", "leaf_10833", "leaf_11188", "leaf_11172",
        "leaf_10653", "leaf_10657",
      },
    },
    {
      id = "lite_xl_quality_candidate",
      claim = "Selected crash-recovery/performance/breadth experiments "
        .. "layered over the core envelope without changing core support "
        .. "truth; empty until an experiment-class leaf lands.",
      admitted_classes = {
        security = true, protocol = true, session = true,
        document = true, diagnostic = true, provider = true,
        configuration = true, advertisement = true, quality = true,
      },
      members = {},
    },
    {
      id = "lite_xl_managed_package_candidate",
      claim = "Exact admitted client/language/package tree reserved for "
        .. "the #9010 managed package subject; empty until that owner "
        .. "admits components.",
      admitted_classes = {
        security = true, protocol = true, session = true,
        document = true, diagnostic = true, provider = true,
        configuration = true, advertisement = true, package = true,
      },
      members = {},
    },
  },
}
