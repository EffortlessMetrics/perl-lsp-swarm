-- Actual-host Neovim activation/root envelope for issue #10502.
--
-- Run through scripts/ux/neovim_activation_root_smoke.sh, which builds the
-- deterministic fixture tree and supplies absolute REPO_ROOT, FIXTURE_ROOT,
-- and PERLLSP paths.
--
-- Three rules keep this probe honest:
--
--   * native filetype is recorded before any override, so a user-configured
--     override can never be projected as built-in Neovim behaviour;
--   * attaching is recorded separately from semantic behaviour, so activation
--     cannot be promoted into a support claim;
--   * every root that claims isolation must name its own facts through a
--     server response while the identically-spelled parent/sibling facts stay
--     absent, so `root_dir` equality is never the only oracle.
--
-- The envelope carries normalized roles and content digests rather than the
-- private absolute paths of the machine that produced it.

local SCHEMA_VERSION = 'neovim_activation_root_envelope.v1'

local function required_env(name)
  local value = vim.env[name]
  if not value or value == '' then
    error(('missing required environment variable %s'):format(name))
  end
  return value
end

local function normalize(path)
  return vim.fs.normalize(vim.fn.fnamemodify(path, ':p'))
end

-- Digests are computed by the wrapper: Neovim's string API cannot carry the
-- NUL bytes of a compiled binary.
local function required_digest(name)
  local value = required_env(name)
  if #value ~= 64 or not value:match('^%x+$') then
    error(('environment variable %s is not a sha256 digest'):format(name))
  end
  return value
end

if vim.fn.has('nvim-0.11.3') ~= 1 then
  error('Neovim 0.11.3+ is required for the canonical root-marker contract')
end

vim.cmd('filetype on')

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perllsp = normalize(required_env('PERLLSP'))
local config_relative = 'scripts/ux/neovim/perllsp.lua'
local config_path = repo_root .. '/' .. config_relative
local config = dofile(config_path)

-- Roles, not private absolute paths. The envelope is a durable artifact.
local function role_of(path)
  local normalized = normalize(path)
  if normalized == fixture_root then
    return 'fixture:.'
  end
  local prefix = fixture_root .. '/'
  if normalized:sub(1, #prefix) == prefix then
    return 'fixture:' .. normalized:sub(#prefix + 1)
  end
  return 'external'
end

local envelope = {
  schema_version = SCHEMA_VERSION,
  envelope_version = 1,
  host = {
    family = 'neovim',
    version = tostring(vim.version()),
    os = vim.uv.os_uname().sysname,
    arch = vim.uv.os_uname().machine,
  },
  config = {
    path = config_relative,
    sha256 = required_digest('CONFIG_SHA256'),
    filetypes = config.filetypes,
    root_marker_groups = config.root_markers,
  },
  server = {
    role = 'candidate_build',
    sha256 = required_digest('PERLLSP_SHA256'),
  },
  file_families = {},
  roots = {},
  limitations = {},
  claim_boundary = 'Native Neovim filetype detection, canonical-config activation, '
    .. 'and workspace-root selection only. Provider depth, document lifecycle, '
    .. 'and process supervision are owned by later Neovim stages.',
}

-- ---------------------------------------------------------------------------
-- File-family denominator.
-- ---------------------------------------------------------------------------

-- Each row declares the disposition the project intends. The probe records the
-- observed native filetype and attach state independently, and refuses to emit
-- a row whose observation contradicts its declared disposition.
local FILE_FAMILIES = {
  { id = 'source.pl', file = 'sample.pl', disposition = 'native_perl_and_attached' },
  { id = 'source.pm', file = 'Sample.pm', disposition = 'native_perl_and_attached' },
  { id = 'source.psgi', file = 'app.psgi', disposition = 'native_perl_and_attached' },
  { id = 'source.t', file = 'basic.t', disposition = 'native_perl_and_attached' },
  { id = 'source.PL', file = 'legacy.PL', disposition = 'native_perl_and_attached' },
  {
    id = 'shebang.cgi',
    file = 'handler.cgi',
    disposition = 'native_perl_and_attached',
    content_dependent = true,
  },
  {
    id = 'shebang.fcgi',
    file = 'handler.fcgi',
    disposition = 'native_perl_and_attached',
    content_dependent = true,
  },
  {
    id = 'shebang.bin_tool',
    file = 'bin/tool',
    disposition = 'native_perl_and_attached',
    content_dependent = true,
  },
  {
    id = 'shebang.script_tool',
    file = 'script/tool',
    disposition = 'native_perl_and_attached',
    content_dependent = true,
  },
  -- Negative control for the rows above: same suffix as `shebang.cgi`, no
  -- interpreter line, therefore no native Perl classification.
  {
    id = 'suffix_only.cgi',
    file = 'plain.cgi',
    disposition = 'native_nonperl_override_possible',
    content_dependent = true,
    reason = 'Neovim classifies .cgi from the interpreter line; without one the '
      .. 'suffix alone does not select perl. Attaching perllsp here is a user or '
      .. 'project override, not built-in behaviour.',
  },
  {
    id = 'metadata.cpanfile',
    file = 'cpanfile',
    disposition = 'native_nonperl_override_possible',
    reason = 'cpanfile is Perl-syntax project metadata with no native Neovim '
      .. 'filetype. Users may map it to perl; the canonical config does not.',
  },
  {
    id = 'adjacent.pod',
    file = 'Doc.pod',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'POD has its own native filetype and is documentation rather than a '
      .. 'Perl compilation unit.',
  },
  {
    id = 'adjacent.xs',
    file = 'Native.xs',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'XS has its own native filetype and is C with a Perl-facing preamble.',
  },
  {
    id = 'template.tt',
    file = 'template.tt',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'Template Toolkit source is a mixed-language document; forcing perl '
      .. 'would misclassify the whole file.',
  },
  {
    id = 'template.tt2',
    file = 'template.tt2',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'Template Toolkit source is a mixed-language document; forcing perl '
      .. 'would misclassify the whole file.',
  },
  {
    id = 'template.ep',
    file = 'view.ep',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'Embedded Perl templates keep a template filetype; the Perl regions '
      .. 'are not a standalone compilation unit.',
  },
  {
    id = 'template.mason',
    file = 'view.mason',
    disposition = 'intentionally_adjacent_or_mixed',
    reason = 'Mason components keep a template filetype; the Perl regions are not '
      .. 'a standalone compilation unit.',
  },
}

-- Contract violations are collected rather than thrown, so a failing run still
-- emits a complete envelope naming exactly which rows failed and why.
local failures = {}

local function eligible_for_config(filetype)
  for _, candidate in ipairs(config.filetypes) do
    if candidate == filetype then
      return true
    end
  end
  return false
end

-- Returns the client only when it reached `initialized` inside the budget.
--
-- `vim.wait` leaves the discovered client in scope even when it times out, so
-- returning it unconditionally would let a server that attached but never
-- finished initializing be recorded as a successful activation.
local function wait_for_client(bufnr, timeout_ms)
  local client
  local initialized = vim.wait(timeout_ms, function()
    client = vim.lsp.get_clients({ bufnr = bufnr, name = 'perllsp' })[1]
    return client ~= nil and client.initialized
  end, 25)
  if not initialized then
    return nil
  end
  return client
end

-- Record the native denominator before the canonical config is registered, so
-- nothing in the activation stage can influence the detection result.
local native_filetypes = {}
for _, family in ipairs(FILE_FAMILIES) do
  local path = fixture_root .. '/filetypes/' .. family.file
  local bufnr = vim.fn.bufadd(path)
  vim.fn.bufload(bufnr)
  native_filetypes[family.id] = vim.filetype.match({ filename = path, buf = bufnr }) or ''
  vim.api.nvim_buf_delete(bufnr, { force = true })
end

-- Use the exact checked config for actual activation. Only the executable path
-- is overridden, so the envelope cannot silently describe an ambient binary.
config = vim.deepcopy(config)
config.cmd = { perllsp, '--stdio' }
vim.lsp.config('perllsp', config)
vim.lsp.enable('perllsp')

for _, family in ipairs(FILE_FAMILIES) do
  local path = fixture_root .. '/filetypes/' .. family.file
  local native = native_filetypes[family.id]
  vim.cmd('edit ' .. vim.fn.fnameescape(path))

  local opened_filetype = vim.bo.filetype
  local config_eligible = eligible_for_config(opened_filetype)
  -- Only an eligible buffer can ever attach, so a non-eligible row needs a
  -- short settle rather than the full readiness budget.
  local client = wait_for_client(0, config_eligible and 15000 or 750)

  local row = {
    fixture = family.file,
    native_filetype = native,
    opened_filetype = opened_filetype,
    config_eligible = config_eligible,
    attached = client ~= nil,
    language_id = client ~= nil and opened_filetype or '',
    override_applied = false,
    content_dependent = family.content_dependent == true,
    disposition = family.disposition,
  }
  if family.reason then
    row.reason = family.reason
  end

  -- A contradicted row is downgraded to `not_proven` and keeps its evidence,
  -- rather than aborting the run: the envelope stays a complete denominator and
  -- the exit code below still fails closed.
  local contradiction = nil
  if family.disposition == 'native_perl_and_attached' then
    if native ~= 'perl' then
      contradiction = ('expected native perl detection, observed %q'):format(native)
    elseif not row.attached then
      contradiction = 'detected as perl but perllsp did not attach'
    end
  elseif native == 'perl' then
    contradiction = ('declared %s but Neovim natively detected perl'):format(family.disposition)
  elseif row.attached then
    contradiction =
      ('declared %s but attached through the canonical config'):format(family.disposition)
  end

  if contradiction then
    row.disposition = 'not_proven'
    row.reason = contradiction
    table.insert(failures, ('file family %s: %s'):format(family.id, contradiction))
  end

  envelope.file_families[family.id] = row
end

-- ---------------------------------------------------------------------------
-- Root matrix.
--
-- `expected_absent` names the identically-spelled facts that live in a parent
-- or sibling root. A row only passes when the selected root's own symbol is
-- returned and every one of those competing symbols stays absent.
-- ---------------------------------------------------------------------------

local ROOT_CASES = {
  {
    id = 'marker.perl_lsp_toml',
    marker = '.perl-lsp.toml',
    root = 'roots/marker-dot',
    marker_value = 'markerdot',
    expected_absent = {},
  },
  {
    id = 'marker.build_pl',
    marker = 'Build.PL',
    root = 'roots/marker-build',
    marker_value = 'markerbuild',
    expected_absent = {},
  },
  {
    id = 'marker.dist_ini',
    marker = 'dist.ini',
    root = 'roots/marker-dist',
    marker_value = 'markerdist',
    expected_absent = {},
  },
  {
    id = 'conflict.nearest_perl_marker_beats_farther',
    marker = 'Makefile.PL',
    root = 'roots/nearest-perl/sub',
    marker_value = 'nearestperl',
    expected_absent = { 'outerperl' },
  },
  {
    id = 'conflict.perl_marker_beats_git',
    marker = 'cpanfile',
    root = 'roots/perl-beats-git/app',
    marker_value = 'appperl',
    expected_absent = { 'gitrootperl' },
  },
  {
    id = 'conflict.competing_markers_at_depth',
    marker = 'cpanfile',
    root = 'roots/depth-conflict/nested/deep',
    marker_value = 'deepdepth',
    expected_absent = { 'shallowdepth' },
  },
  {
    id = 'isolation.sibling_same_relative_path',
    marker = 'cpanfile',
    root = 'roots/siblings/alpha',
    marker_value = 'siblingalpha',
    expected_absent = { 'siblingbeta' },
  },
  {
    id = 'fallback.git_only',
    marker = '.git',
    root = 'roots/git-only',
    marker_value = 'gitonly',
    expected_absent = {},
  },
  {
    id = 'fallback.git_file_linked_worktree',
    marker = '.git',
    root = 'roots/worktree-linked',
    marker_value = 'worktreelinked',
    expected_absent = { 'worktreesource' },
  },
}

-- Every marker the canonical config can select must win a root case here.
--
-- Without this the matrix silently stops covering a marker the moment one is
-- added to `perllsp.lua`: the run would stay green while an unexercised marker
-- shipped as if it were proven. The groups are read, not flattened in place —
-- `config.root_markers` is emitted verbatim as `root_marker_groups`, and the
-- nesting is what makes the Perl markers equal priority above `.git`.
local function configured_markers()
  local markers = {}
  for _, group in ipairs(config.root_markers) do
    if type(group) == 'table' then
      for _, marker in ipairs(group) do
        markers[marker] = true
      end
    else
      markers[group] = true
    end
  end
  return markers
end

-- The no-marker boundary cell is deliberately outside this denominator: it
-- exercises the absence of every marker rather than any one of them.
local function report_uncovered_markers(proven_markers)
  local uncovered = {}
  for marker in pairs(configured_markers()) do
    if not proven_markers[marker] then
      table.insert(uncovered, marker)
    end
  end
  table.sort(uncovered)
  for _, marker in ipairs(uncovered) do
    table.insert(
      failures,
      ('configured root marker %s: no root case proved it'):format(marker)
    )
  end
end

local function request(client, bufnr, method, params)
  local response = client:request_sync(method, params, 10000, bufnr)
  if response == nil then
    return nil, 'no response'
  end
  if response.err then
    return nil, vim.inspect(response.err)
  end
  return response.result, nil
end

-- Builds one root row. A row that cannot be established returns `not_proven`
-- with its reason instead of aborting, so the envelope keeps the complete
-- denominator even when a case fails.
local function evaluate_root_case(case)
  local entry = fixture_root .. '/' .. case.root .. '/t/probe.pl'
  local expected_root = normalize(fixture_root .. '/' .. case.root)
  local expected_module = expected_root .. '/lib/RootProbe.pm'

  local function unestablished(reason)
    return {
      marker = case.marker,
      expected_role = role_of(expected_root),
      actual_role = 'none',
      root_match = false,
      semantic = {
        definition_method = 'textDocument/definition',
        content_method = 'workspace/symbol',
        expected_target_role = role_of(expected_module),
        observed_target_role = 'none',
        expected_marker = case.marker_value,
        observed_marker = '',
        rejected_symbols = {},
        outcome = 'not_proven',
        reason = reason,
      },
    },
      reason
  end

  vim.cmd('edit ' .. vim.fn.fnameescape(entry))
  if vim.bo.filetype ~= 'perl' then
    return unestablished(('root fixture did not detect as perl, got %q'):format(vim.bo.filetype))
  end

  local client = wait_for_client(0, 15000)
  if not client then
    return unestablished('perllsp did not attach')
  end

  local actual_root = client.root_dir and normalize(client.root_dir) or ''

  -- Structural oracle: the module reference resolves inside the selected root.
  local definition, definition_error = request(client, 0, 'textDocument/definition', {
    textDocument = { uri = vim.uri_from_fname(entry) },
    position = { line = 2, character = 6 },
  })
  -- `textDocument/definition` may answer with a single Location, an array of
  -- Location, or LocationLink entries that name the target as `targetUri`.
  -- Anything else is a malformed success and must not be indexed blindly.
  local definition_uri = ''
  local malformed = nil
  if definition ~= nil and type(definition) ~= 'table' then
    malformed = ('textDocument/definition returned %s, not a Location'):format(type(definition))
  else
    local location = definition and (definition[1] or definition) or nil
    if location ~= nil and type(location) ~= 'table' then
      malformed = ('textDocument/definition returned a %s entry'):format(type(location))
    else
      local target_uri = location and (location.uri or location.targetUri) or nil
      if target_uri ~= nil and type(target_uri) ~= 'string' then
        malformed = 'textDocument/definition target uri is not a string'
      elseif target_uri then
        definition_uri = normalize(vim.uri_to_fname(target_uri))
      end
    end
  end

  -- Content oracle: the indexed symbol names which root actually won. The
  -- identically-named `probe_marker` exists in every candidate root, so only
  -- the root-unique symbol can discriminate.
  local symbols, symbol_error = request(client, 0, 'workspace/symbol', { query = 'probe_' })
  local observed_symbols = {}
  local observed_marker = ''
  if symbols ~= nil and type(symbols) ~= 'table' then
    malformed = malformed
      or ('workspace/symbol returned %s, not a symbol list'):format(type(symbols))
  else
    for _, symbol in ipairs(symbols or {}) do
      if type(symbol) == 'table' and type(symbol.name) == 'string' then
        observed_symbols[symbol.name] = true
        if symbol.name == 'probe_' .. case.marker_value then
          observed_marker = case.marker_value
        end
      else
        malformed = malformed or 'workspace/symbol returned an entry with no name'
      end
    end
  end

  local rejected = {}
  local leaked = nil
  for _, absent in ipairs(case.expected_absent) do
    if observed_symbols['probe_' .. absent] then
      leaked = absent
    end
    table.insert(rejected, 'probe_' .. absent)
  end

  local outcome = 'proven'
  local reason = nil
  if malformed then
    outcome = 'not_proven'
    reason = malformed
  elseif actual_root ~= expected_root then
    outcome = 'not_proven'
    reason = ('selected root %s; expected %s'):format(role_of(actual_root), role_of(expected_root))
  elseif leaked then
    outcome = 'not_proven'
    reason = ('a fact from the wrong root was indexed: probe_%s'):format(leaked)
  elseif definition_uri ~= expected_module then
    outcome = 'not_proven'
    reason = ('definition resolved to %s; expected %s (%s)'):format(
      role_of(definition_uri),
      role_of(expected_module),
      definition_error or 'no error reported'
    )
  elseif observed_marker ~= case.marker_value then
    outcome = 'not_proven'
    reason = ('root-unique symbol probe_%s was not indexed (%s)'):format(
      case.marker_value,
      symbol_error or 'no error reported'
    )
  end

  local row = {
    marker = case.marker,
    expected_role = role_of(expected_root),
    actual_role = role_of(actual_root),
    root_match = actual_root == expected_root,
    semantic = {
      definition_method = 'textDocument/definition',
      content_method = 'workspace/symbol',
      expected_target_role = role_of(expected_module),
      observed_target_role = role_of(definition_uri),
      expected_marker = case.marker_value,
      observed_marker = observed_marker,
      rejected_symbols = rejected,
      outcome = outcome,
    },
  }
  if reason then
    row.semantic.reason = reason
  end

  return row, reason
end

local proven_markers = {}
for _, case in ipairs(ROOT_CASES) do
  local row, reason = evaluate_root_case(case)
  envelope.roots[case.id] = row
  if reason then
    table.insert(failures, ('root case %s (%s): %s'):format(case.id, case.marker, reason))
  else
    proven_markers[case.marker] = true
  end
end

-- Coverage is judged on proven rows, not on the case list: a marker whose only
-- case failed is no better covered than one that was never exercised.
report_uncovered_markers(proven_markers)

-- Single-file/no-marker behaviour stays an observed cell rather than an
-- assumption: it is recorded, not asserted, and carries no semantic claim.
local single_file = fixture_root .. '/nomarker/single.pl'
vim.cmd('edit ' .. vim.fn.fnameescape(single_file))
local single_client = wait_for_client(0, 15000)
local single_role = single_client
    and single_client.root_dir
    and role_of(single_client.root_dir)
  or 'none'
envelope.roots['boundary.no_marker_single_file'] = {
  marker = 'none',
  expected_role = 'observation_only',
  actual_role = single_role,
  -- There is no root expectation to match here, and claiming one would be the
  -- overclaim this row exists to avoid.
  root_match = single_role == 'observation_only',
  semantic = {
    definition_method = 'textDocument/definition',
    content_method = 'workspace/symbol',
    expected_target_role = 'observation_only',
    observed_target_role = 'observation_only',
    expected_marker = '',
    observed_marker = '',
    rejected_symbols = {},
    outcome = 'not_applicable',
    reason = ('single-file support policy is owned by #7743; this row records that '
      .. 'perllsp attached=%s without claiming a root-isolation result'):format(
      tostring(single_client ~= nil)
    ),
  },
}

envelope.limitations = {
  'Filetype dispositions describe this Neovim build only; #7716 owns the version matrix.',
  'Adjacent and template families are recorded as activation policy, not as evidence '
    .. 'about Perl regions inside mixed-language documents.',
  'Client shutdown below is fixture cleanup; graceful-exit and OS-child settlement are '
    .. 'owned by the later process-supervision stage.',
  'docs/EDITORS and integrations/neovim keep their own copies of the marker list for '
    .. 'reader and upstream-submission purposes; #7722 and #7736 own reconciling those.',
}

for _, client in ipairs(vim.lsp.get_clients()) do
  client:stop(true)
end
vim.wait(2000, function()
  return #vim.lsp.get_clients() == 0
end, 20)

-- The envelope is emitted whatever the outcome, so a failing run still hands
-- its consumers the complete denominator and the exact reasons.
io.stdout:write(vim.json.encode(envelope) .. '\n')

if #failures > 0 then
  for _, failure in ipairs(failures) do
    io.stderr:write('NOT_PROVEN: ' .. failure .. '\n')
  end
  os.exit(2)
end
