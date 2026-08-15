-- Actual-host Neovim activation/root receipt for issue #7743.
--
-- Run through scripts/ux/neovim_activation_root_smoke.sh. The wrapper creates
-- deterministic fixture files and supplies absolute REPO_ROOT, FIXTURE_ROOT,
-- and PERLLSP paths.

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

local function record(receipt, key, value)
  receipt[key] = value
end

local function detect_filetype(path)
  local bufnr = vim.fn.bufadd(path)
  vim.fn.bufload(bufnr)
  local filetype = vim.filetype.match({ filename = path, buf = bufnr })
  vim.api.nvim_buf_delete(bufnr, { force = true })
  return filetype or ''
end

local function wait_for_client(bufnr)
  local client
  local ok = vim.wait(8000, function()
    local clients = vim.lsp.get_clients({ bufnr = bufnr, name = 'perllsp' })
    client = clients[1]
    return client ~= nil and client.initialized
  end, 20)
  if not ok then
    return nil
  end
  return client
end

if vim.fn.has('nvim-0.11.3') ~= 1 then
  error('Neovim 0.11.3+ is required for the canonical root-marker contract')
end

vim.cmd('filetype on')

local repo_root = normalize(required_env('REPO_ROOT'))
local fixture_root = normalize(required_env('FIXTURE_ROOT'))
local perllsp = normalize(required_env('PERLLSP'))
local config_path = repo_root .. '/scripts/ux/neovim/perllsp.lua'
local config = dofile(config_path)

local receipt = {
  nvim = vim.version(),
  perllsp = perllsp,
  config = config_path,
  filetypes = {},
  roots = {},
}

-- Record the actual native Neovim filetype denominator. Only the stable core
-- source forms are hard assertions here. Test/CGI/legacy and adjacent families
-- are deliberately evidence-only so #7743/#7716 can promote them from the
-- actual supported-host result rather than baking our expectation into the
-- probe itself.
local filetype_cases = {
  { 'sample.pl', true },
  { 'Sample.pm', true },
  { 'app.psgi', true },
  { 'basic.t', false },
  { 'legacy.PL', false },
  { 'handler.cgi', false },
  { 'handler.fcgi', false },
  { 'cpanfile', false },
  { 'bin/tool', false },
  { 'Doc.pod', false },
  { 'Native.xs', false },
  { 'template.tt', false },
}

for _, case in ipairs(filetype_cases) do
  local relative, require_perl = case[1], case[2]
  local path = fixture_root .. '/filetypes/' .. relative
  local filetype = detect_filetype(path)
  receipt.filetypes[relative] = {
    detected = filetype,
    required_for_base_contract = require_perl,
  }
  if require_perl and filetype ~= 'perl' then
    error(('expected native Neovim filetype=perl for %s, got %q'):format(relative, filetype))
  end
end

-- Use the exact checked config for actual LSP activation. Override only the
-- executable path so the receipt cannot accidentally use an ambient binary.
config = vim.deepcopy(config)
config.cmd = { perllsp, '--stdio' }
vim.lsp.config('perllsp', config)
vim.lsp.enable('perllsp')

-- Every configured root marker gets a winning actual-host case. The first two
-- also prove the priority rules: equal-priority Perl markers choose the nearest
-- project, while `.git` remains the lower-priority fallback.
local root_cases = {
  {
    name = 'makefile-pl-nearest-perl-marker',
    marker = 'Makefile.PL',
    file = fixture_root .. '/outer/sub/lib/Nearest.pm',
    expected = fixture_root .. '/outer/sub',
  },
  {
    name = 'cpanfile-before-git-fallback',
    marker = 'cpanfile',
    file = fixture_root .. '/gitroot/app/lib/App.pm',
    expected = fixture_root .. '/gitroot/app',
  },
  {
    name = 'dot-perl-lsp-toml',
    marker = '.perl-lsp.toml',
    file = fixture_root .. '/marker-dot/lib/DotConfig.pm',
    expected = fixture_root .. '/marker-dot',
  },
  {
    name = 'build-pl',
    marker = 'Build.PL',
    file = fixture_root .. '/marker-build/lib/BuildRoot.pm',
    expected = fixture_root .. '/marker-build',
  },
  {
    name = 'dist-ini',
    marker = 'dist.ini',
    file = fixture_root .. '/marker-dist/lib/DistRoot.pm',
    expected = fixture_root .. '/marker-dist',
  },
  {
    name = 'git-only-fallback',
    marker = '.git',
    file = fixture_root .. '/git-only/lib/GitOnly.pm',
    expected = fixture_root .. '/git-only',
  },
}

for _, case in ipairs(root_cases) do
  vim.cmd('edit ' .. vim.fn.fnameescape(case.file))
  if vim.bo.filetype ~= 'perl' then
    error(('expected opened root fixture %s to detect as perl, got %q'):format(case.file, vim.bo.filetype))
  end

  local client = wait_for_client(0)
  if not client then
    error(('perllsp did not attach for root case %s'):format(case.name))
  end

  local actual_root = client.root_dir and normalize(client.root_dir) or ''
  local expected_root = normalize(case.expected)
  receipt.roots[case.name] = {
    marker = case.marker,
    expected = expected_root,
    actual = actual_root,
    client_id = client.id,
  }

  if actual_root ~= expected_root then
    error(
      ('root case %s (%s) selected %q; expected %q'):format(
        case.name,
        case.marker,
        actual_root,
        expected_root
      )
    )
  end
end

-- Single-file/no-marker behavior is an observed support cell rather than an
-- assumption. The receipt records whether Neovim attaches and which root (if
-- any) it selects; #7743 can use this to settle the final upstream config.
local single_file = fixture_root .. '/nomarker/single.pl'
vim.cmd('edit ' .. vim.fn.fnameescape(single_file))
local single_client = wait_for_client(0)
record(receipt.roots, 'single-file-no-marker', {
  attached = single_client ~= nil,
  actual = single_client and single_client.root_dir and normalize(single_client.root_dir) or '',
})

for _, client in ipairs(vim.lsp.get_clients()) do
  client:stop(true)
end
vim.wait(2000, function()
  return #vim.lsp.get_clients() == 0
end, 20)

io.stdout:write(vim.json.encode(receipt) .. '\n')
