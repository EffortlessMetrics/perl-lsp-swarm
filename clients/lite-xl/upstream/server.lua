-- Class in charge of establishing communication with an LSP server and
-- managing requests, notifications and responses from both the server
-- and the client that is establishing the connection.
--
-- @copyright Jefferson Gonzalez
-- @license MIT
-- @inspiration: https://github.com/orbitalquark/textadept-lsp
--
-- LSP Documentation:
-- https://microsoft.github.io/language-server-protocol/specifications/specification-3-17

-- Staged exact upstream source for the lite-xl integration train.
-- Upstream subject: lite-xl/lite-xl-lsp server.lua
--   base ref : d1432ae0736cd9531798b4bc1221835f534cc689
--   base blob: 33c8ccae7362ddb01aa980bff024a4ef1682c8f9
-- Local patch (#11151): inbound LSP framing is owned by one explicit bounded
-- frame reader. Header accumulation is capped, exactly one decimal
-- Content-Length is required (missing/malformed/signed/overflowing/duplicate
-- declarations fail closed), declared bodies are bounded and read as exact
-- bytes, remainders stay owned by the parser across arbitrary read
-- boundaries, and every terminal framing fault raises one typed failure
-- object carrying bounded numeric metadata only - never accumulated frame or
-- body content. Framing failures stay distinct from JSON syntax failures.

local json = require "plugins.lsp.json"
local util = require "plugins.lsp.util"
-- Local patch (#11172): the initialize advertisement is folded from the
-- profile-scoped capability manifest, so the wire payload can never exceed
-- the reconciled capability/affordance matrix.
local capability_manifest = require "plugins.lsp.capability_manifest"
local Object = require "core.object"

---Visible truncation marker appended by bounded text excerpts (#11155).
local BOUNDED_TEXT_MARKER = "...[truncated]"

---Bounded text excerpt with a visible truncation marker (#11155). Default and
---failure logs use this so error transport text cannot grow without bound.
local function bound_text(text, limit)
  text = tostring(text)
  limit = limit or 200
  if #text <= limit then
    return text
  end
  return text:sub(1, limit) .. BOUNDED_TEXT_MARKER
end

---Deterministic 32-bit FNV-1a content digest as eight hex characters
---(#11155). The multiplication is split through 16-bit halves so every
---intermediate stays below 2^53: integer runtimes wrap exactly and pure-double
---runtimes (LuaJIT, Lua 5.1/5.2) never round the low bits, giving every
---runtime family member the same value. Used to give failure logs content
---identity without retaining any content.
local function content_digest(data)
  local hash = 2166136261
  for i = 1, #data do
    local byte = string.byte(data, i)
    local low = hash % 65536
    local high = math.floor(hash / 65536)
    hash = (
      low * 16777619
      + (high * 16777619) % 65536 * 65536
      + byte
    ) % 4294967296
  end
  return string.format("%08x", hash)
end

---@alias lsp.server.callback fun(server: lsp.server, ...)
---@alias lsp.server.timeoutcb fun(server: lsp.server, ...)
---@alias lsp.server.notificationcb fun(server: lsp.server, params: table)
---@alias lsp.server.responsecb fun(server: lsp.server, response: table, request?: lsp.server.request)

---@class lsp.server.languagematch
---@field id string
---@field pattern string

---@class lsp.server.request
---@field id integer
---@field method string
---@field data table|nil
---@field params table
---@field callback lsp.server.responsecb | nil
---@field overwritten boolean
---@field overwritten_callback lsp.server.responsecb | nil
---@field sending boolean
---@field raw_data string
---@field timeout number
---@field timeout_callback lsp.server.timeoutcb | nil
---@field timestamp number
---@field times_sent integer

---LSP Server communication library.
---@class lsp.server : core.object
---@field public name string
---@field public language string | lsp.server.languagematch[]
---@field public file_patterns table
---@field public current_request integer
---@field public init_options table
---@field public settings table | nil
---@field public event_listeners table
---@field public message_listeners table
---@field public request_listeners table
---@field public request_list lsp.server.request[]
---@field public response_list table
---@field public notification_list lsp.server.request[]
---@field public raw_list lsp.server.request[]
---@field public command table
---@field public write_fails integer
---@field public write_fails_before_shutdown integer
-- Explicit opt-in local protocol trace (#11155): when enabled, verbose logs
-- carry complete protocol payloads and may therefore contain source code,
-- local paths, configuration values and anything the server emits. Disabled
-- by default; never enable it for canonical host or CI proof artifacts.
---@field public verbose boolean
---@field public initialized boolean
---@field public max_queued_requests integer
---@field public proc process | nil
---@field public quit_timeout number
---@field public exit_timer lsp.timer | nil
---@field public capabilities table
---@field public custom_capabilities table
---@field public yield_on_reads boolean
---@field public running boolean
local Server = Object:extend()

---LSP Server constructor options
---@class lsp.server.options
---@field name string
---@field language string | lsp.server.languagematch[]
---@field file_patterns table<integer, string>
---@field string|command table<integer, string>
---@field quit_timeout number
---@field windows_skip_cmd boolean
---@field env table<string, string>
---@field settings table
---@field init_options table
---@field custom_capabilities table
---@field on_start? fun(server: lsp.server)
---@field requests_per_second number
---@field incremental_changes boolean
Server.options = {
  ---Name of the server
  name = "",
  ---Programming language identifier.
  ---Can be a string or a table.
  ---If the table is empty, the file extension will be used instead.
  ---The table should be an array of tables containing `id` and `pattern`.
  ---The `pattern` will be matched with the file path.
  ---Will use the `id` of the first `pattern` that matches.
  ---If no pattern matches, the file extension will be used instead.
  language = {},
  ---Patterns to match the language files
  file_patterns = {},
  ---Command to launch LSP server and optional arguments
  command = {},
  ---On Windows, avoid running the LSP server with cmd.exe
  windows_skip_cmd = false,
  ---Enviroment variables to set for the server command
  env = {},
  ---Seconds before closing the server when not needed anymore
  quit_timeout = 60,
  ---Optional table of settings to pass into the LSP
  ---Note that also having a settings.json or settings.lua in
  ---your workspace directory is supported
  settings = {},
  ---Optional table of initializationOptions for the LSP
  init_options = {},
  ---Optional table of capabilities that will be merged with our default one
  custom_capabilities = {},
  ---Function called when the server has been started
  on_start = nil,
  ---Some servers like bash language server support incremental changes
  ---which are more performant but don't advertise it, set to true to force
  ---incremental changes even if server doesn't advertise them
  incremental_changes = false,
  ---True to debug the lsp client when developing it
  verbose = false,
}

---Default bound on simultaneously queued outbound requests (#10833). Rate
---control moved from enqueue admission (the old hit-rate dropper silently
---discarded load-bearing traffic) to send scheduling: messages are queued or
---coalesced, and only an explicit bound breach without an applicable
---coalescing policy rejects with a typed not_queued disposition.
---@type integer
Server.MAX_QUEUED_REQUESTS = 256

---Default timeout when sending a request to lsp server.
---@type integer Time in seconds
Server.DEFAULT_TIMEOUT = 10

---The maximum amount of data to retrieve when reading from server.
---@type integer Amount of bytes
Server.BUFFER_SIZE = 1024 * 10

---Maximum bytes scanned while looking for the CRLFCRLF header terminator
---(#11151). LSP headers are short control metadata; 16 KiB leaves generous
---room for optional headers while bounding adversarial or corrupt header
---streams. Exceeding it fails closed.
---@type integer Amount of bytes
Server.MAX_HEADER_BYTES = 16 * 1024

---Maximum declared inbound body size accepted from the server process
---(#11151). Bounds the memory and CPU work a crashed, misconfigured or
---compromised child can drive through one frame. Sized generously above
---current perllsp response envelopes (completion/hover/symbol results are
---kilobytes to low megabytes); #10722 owns measured envelope evidence and may
---raise it with that evidence. It is configurable per server via
---options.max_body_bytes but never disabled.
---@type integer Amount of bytes
Server.MAX_BODY_BYTES = 64 * 1024 * 1024

---Maximum stderr bytes retained by read_errors (#11155). Draining always
---runs to completion so the child can never block on a full pipe; bytes past
---this bound are discarded and the truncation is marked visibly.
---@type integer Amount of bytes
Server.MAX_STDERR_BYTES = 16 * 1024

---LSP Docs: /#errorCodes
Server.error_code = {
  ParseError                      = -32700,
  InvalidRequest                  = -32600,
  MethodNotFound                  = -32601,
  InvalidParams                   = -32602,
  InternalError                   = -32603,
  jsonrpcReservedErrorRangeStart  = -32099,
  serverErrorStart                = -32099,
  ServerNotInitialized            = -32002,
  UnknownErrorCode                = -32001,
  jsonrpcReservedErrorRangeEnd    = -32000,
  serverErrorEnd                  = -32000,
  lspReservedErrorRangeStart      = -32899,
  ContentModified                 = -32801,
  RequestCancelled                = -32800,
  lspReservedErrorRangeEnd        = -32800,
}

---LSP Docs: /#completionTriggerKind
Server.completion_trigger_Kind = {
  Invoked = 1,
  TriggerCharacter = 2,
  TriggerForIncompleteCompletions = 3
}

---LSP Docs: /#diagnosticSeverity
Server.diagnostic_severity = {
  Error = 1,
  Warning = 2,
  Information = 3,
  Hint = 4
}

---LSP Docs: /#textDocumentSyncKind
Server.text_document_sync_kind = {
  None = 0,
  Full = 1,
  Incremental = 2
}

---LSP Docs: /#completionItemKind
Server.completion_item_kind = {
  'Text', 'Method', 'Function', 'Constructor', 'Field', 'Variable', 'Class',
  'Interface', 'Module', 'Property', 'Unit', 'Value', 'Enum', 'Keyword',
  'Snippet', 'Color', 'File', 'Reference', 'Folder', 'EnumMember',
  'Constant', 'Struct', 'Event', 'Operator', 'TypeParameter'
}

---LSP Docs: /#symbolKind
Server.symbol_kind = {
  'File', 'Module', 'Namespace', 'Package', 'Class', 'Method', 'Property',
  'Field', 'Constructor', 'Enum', 'Interface', 'Function', 'Variable',
  'Constant', 'String', 'Number', 'Boolean', 'Array', 'Object', 'Key',
  'Null', 'EnumMember', 'Struct', 'Event', 'Operator', 'TypeParameter'
}

---LSP Docs: /#insertTextFormat
Server.insert_text_format = {
  PlainText = 1,
  Snippet = 2
}

---LSP Docs: /#messageType
---@enum
Server.message_type = {
	Error = 1,
	Warning = 2,
	Info = 3,
	Log = 4,
	Debug = 5
}

---LSP Docs: /#positionEncodingKind
---@enum
Server.position_encoding_kind = {
  UTF8  = 'utf-8',
  UTF16 = 'utf-16',
  UTF32 = 'utf-32'
}

---@class lsp.server.requestoptions
---@field params? table<string,any>
---@field data? table @Optional data appended to request.
---@field callback? lsp.server.responsecb @Default callback executed when a response is received.
---@field overwrite? boolean @Substitute same previous request with new one if not sent.
---@field overwritten_callback? lsp.server.responsecb @Executed in place of original response callback if the request should have been overwritten but was already sent.
---@field raw_data? string @Request body used when sending a raw request.
---@field timeout? number @Timeout in seconds to consider the request unanswered.
---@field timeout_callback? lsp.server.timeoutcb @Callback executed when the request times out.

---Get a completion kind label from its id or empty string if not found.
---@param id integer
---@return string
function Server.get_completion_item_kind(id)
  return Server.completion_item_kind[id] or ""
end

---Get list of completion kinds.
---@return table
function Server.get_completion_items_kind_list()
  local list = {}
  for i = 1, #Server.completion_item_kind do
    if i ~= 15 then --Disable snippets
      table.insert(list, i)
    end
  end

  return list
end

---Get a symbol kind label from its id or empty string if not found.
---@param id integer
---@return string
function Server.get_symbol_kind(id)
  return Server.symbol_kind[id] or ""
end

---Get list of symbol kinds.
---@return table
function Server.get_symbols_kind_list()
  local list = {}
  for i = 1, #Server.symbol_kind do
    list[i] = i
  end

  return list
end

---Given a ServerCapabilities object, return a "normalized" version
---that simplifies capabilities checks.
---@param capabilities table
---returns table
function Server.normalize_server_capabilities(capabilities)
  local cap = util.deep_merge({ }, capabilities)
  local tds = {
    openClose = false,
    change = false,
    willSave = false,
    willSaveWaitUntil = false,
    save = false
  }
  if cap.textDocumentSync then
    if type(cap.textDocumentSync) ~= "table" then
      -- Convert TextDocumentSyncKind into TextDocumentSyncOptions
      tds = util.deep_merge(tds, {
        openClose = true,
        change = cap.textDocumentSync,
        save = {
          includeText = false
        }
      })
      cap.textDocumentSync = nil
    else
      tds = util.deep_merge(tds, cap.textDocumentSync)
      if type(tds.save) ~= "table" and tds.save then
        tds.save = {
          includeText = false
        }
      end
    end
  end
  cap.textDocumentSync = util.deep_merge(cap.textDocumentSync, tds)
  return cap
end

---Instantiates a new LSP server.
---@param options lsp.server.options
function Server:new(options)
  Server.super.new(self)

  self.name = options.name
  self.language = options.language
  self.file_patterns = options.file_patterns
  self.current_request = 0
  self.init_options = options.init_options or {}
  self.settings = options.settings or nil
  self.event_listeners = {}
  self.message_listeners = {}
  self.request_listeners = {}
  self.request_list = {}
  self.response_list = {}
  self.notification_list = {}
  self.raw_list = {}
  -- Response-obligation correlation (#10785): ids of accepted server
  -- requests still awaiting their handler's terminal reply, and ids whose
  -- terminal reply is already queued. Both die with the generation.
  self.pending_response_ids = {}
  self.answered_response_ids = {}
  self.command = options.command
  self.write_fails = 0
  self.fatal_error = false
  self.snippets = options.snippets
  self.fake_snippets = options.fake_snippets or false
  -- TODO: We may need to lower this but tests so far show that some servers
  -- may actually fail to write many of the request sent to it if it is
  -- indexing the workspace source code or other heavy tasks.
  self.write_fails_before_shutdown = 60
  self.verbose = options.verbose or false
  -- Optional per-server inbound body budget override (#11151); nil keeps the
  -- reviewed Server.MAX_BODY_BYTES default.
  self.max_body_bytes = options.max_body_bytes
  self.last_restart = system.get_time()
  self.initialized = false
  -- Outbound request-queue bound (#10833); see Server.MAX_QUEUED_REQUESTS.
  self.max_queued_requests = Server.MAX_QUEUED_REQUESTS

  self.proc = process.start(
    options.command, {
      stderr = process.REDIRECT_PIPE,
      env = options.env
    }
  )
  self.quit_timeout = options.quit_timeout or 60
  self.exit_timer = nil
  self.capabilities = nil
  self.custom_capabilities = options.custom_capabilities
  self.yield_on_reads = false
  self.incremental_changes = options.incremental_changes or false

  self.read_responses_coroutine = nil

  if options.on_start then options.on_start(self) end
end

---Starts the LSP server process, any listeners should be registered before
---calling this method and this method should be called before any pushes.
---Local patch (#11165): the root URI comes from the one file URI/path
---conversion authority; an unconvertible workspace refuses initialization
---with a typed reason instead of sending a fabricated rootUri.
---@param workspace string
---@param editor_name? string
---@param editor_version? string
---@return boolean initialized
---@return string|nil failure_reason Stable token when not initialized
function Server:initialize(workspace, editor_name, editor_version)
  local root_uri, uri_reason = util.path_to_uri(workspace)
  if not root_uri then
    return false, uri_reason or "unconvertible_workspace"
  end

  self.path = workspace or ""
  self.editor_name = editor_name or "unknown"
  self.editor_version = editor_version or "0.1"

  -- Local patch (#11172): user overrides may claim anything, but claims
  -- over known unimplemented rows are warned about once here instead of
  -- silently becoming support evidence.
  local unsupported_claims =
    capability_manifest.unsupported_custom_claims(self.custom_capabilities)
  if #unsupported_claims > 0 then
    local claimed_paths = {}
    for _, claim in ipairs(unsupported_claims) do
      claimed_paths[#claimed_paths + 1] = claim.path
    end
    self:log(
      "[LSP] warning: custom capabilities claim unimplemented client features: %s",
      table.concat(claimed_paths, ", ")
    )
  end

  self:push_request('initialize', {
    -- Local patch (#10657): initialize pacing is policy-owned. The legacy
    -- hardcoded timeout = 10 was retry spacing under the resend model; with
    -- single-send semantics an explicit short value would terminally expire
    -- the ONLY initialize emission of cold/large-workspace servers, so the
    -- longer INITIALIZE_REQUEST_TIMEOUT policy applies instead (still one
    -- emission under id 1).
    params = {
      processId = system["get_process_id"] and system.get_process_id() or nil,
      clientInfo = {
        name = editor_name or "unknown",
        version = editor_version or "0.1"
      },
      -- TODO: locale
      rootPath = workspace,
      rootUri = root_uri,
      workspaceFolders = {
        {uri = root_uri, name = util.getpathname(workspace)}
      },
      initializationOptions = self.init_options,
      -- Local patch (#11172): capabilities are the manifest's exact truth -
      -- every advertised leaf has an implemented consumer and deterministic
      -- proof; unconsumed leaves (publishDiagnostics relatedInformation,
      -- tagSupport, codeDescriptionSupport) stay absent. User overrides
      -- merge on top as user freedom, never as repository evidence.
      capabilities = util.deep_merge(
        capability_manifest.client_capabilities({
          -- Normalized to a strict boolean: an absent snippets dependency
          -- is a real false, not a missing producer.
          snippet_support = (self.snippets or self.fake_snippets)
            and true or false,
          completion_item_kinds = Server.get_completion_items_kind_list(),
          symbol_kinds = Server.get_symbols_kind_list(),
          position_encoding_list = {
            Server.position_encoding_kind.UTF16
          },
        }),
        self.custom_capabilities
      )
    },
    callback = function(server, response)
      if server.verbose then
        server:log(
          "Processing initialization response:\n%s",
          util.jsonprettify(json.encode(response))
        )
      end
      local result = response.result
      if result then
        server.capabilities = Server.normalize_server_capabilities(result.capabilities)
        server.info = result.serverInfo

        if server.info then
          server:log(
            'Connected to %s %s',
            server.info.name,
            server.info.version or '(unknown version)'
          )
        end

        while not server:notify('initialized') do end -- required by protocol

        -- We wait a few seconds to prevent initialization issues
        coroutine.yield(3)
        server.initialized = true;
        server:send_event_signal("initialized", server, result)
      end
    end
  })

  return true, nil
end

---Register an event listener.
---@param event_name string
---@param callback lsp.server.callback
function Server:add_event_listener(event_name, callback)
  if self.verbose then
    self:log(
      "Listening for event '%s'",
      event_name
    )
  end

  if not self.event_listeners[event_name] then
    self.event_listeners[event_name] = {}
  end
  table.insert(self.event_listeners[event_name], callback)
end

function Server:send_event_signal(event_name, ...)
  if self.event_listeners[event_name] then
    for _, l in ipairs(self.event_listeners[event_name]) do
      l(self, ...)
    end
  else
    self:on_event(event_name)
  end
end

function Server:on_event(event_name)
  if self.verbose then
    self:log("Received event '%s'", event_name)
  end
end

---Send a message to the server that doesn't needs a response.
---@param method string
---@param params? table
---@return boolean sent
function Server:notify(method, params)
  local message = {
    jsonrpc = '2.0',
    method = method,
    params = params or {}
  }

  local data = json.encode(message)

  if self.verbose then
    self:log("Sending notification:\n%s", util.jsonprettify(data))
  end

  local sent, errmsg = self:write_request(data)

  if not sent and self.verbose then
    self:log(
      "Could not send '%s' notification with error: %s",
      method,
      errmsg or "unknown"
    )
  end

  return sent
end

---Reply to a server request.
---@param id integer
---@param result table
---@return boolean sent
function Server:respond(id, result)
  local message = {
    jsonrpc = '2.0',
    id = id,
    result = result
  }

  local data = json.encode(message)

  if self.verbose then
    self:log("Responding to '%s':\n%s", tostring(id), util.jsonprettify(data))
  end

  local sent, errmsg = self:write_request(data)

  if not sent and self.verbose then
    self:log("Could not send response with error: %s", errmsg or "unknown")
  end

  return sent
end

---Respond to a an unknown server request with a method not found error code.
---@param id integer
---@param error_message? string
---@param error_code? integer
---@return boolean sent
function Server:respond_error(id, error_message, error_code)
  local message = {
    jsonrpc = '2.0',
    id = id,
    error = {
      code = error_code or Server.error_code.MethodNotFound,
      message = error_message or "method not found"
    }
  }

  local data = json.encode(message)

  if self.verbose then
    self:log("Responding error to '%s':\n%s", tostring(id), util.jsonprettify(data))
  end

  local sent, errmsg = self:write_request(data)

  if not sent and self.verbose then
    self:log("Could not send response with error: %s", errmsg or "unknown")
  end

  return sent
end

---Sends one of the queued notifications.
function Server:process_notifications()
  if not self.initialized then return end

  -- Clone table as we remove elements while iterating it
  local notifications = {}
  for index, request in ipairs(self.notification_list) do
    notifications[index] = request
  end

  for index, request in ipairs(notifications) do
    request.sending = true
    local message = {
      jsonrpc = '2.0',
      method = request.method,
      params = request.params or {}
    }

    local data = json.encode(message)

    if self.verbose then
        self:log(
          "Sending notification '%s':\n%s",
          request.method,
          util.jsonprettify(data)
        )
    end

    local written, errmsg = self:write_request(data)

    if self.verbose then
      if not written then
        self:log(
          "Failed sending notification '%s' with error: %s",
          request.method,
          errmsg or "unknown"
        )
      end
    end

    if written then
      if request.callback then
        request.callback(self)
      end
      table.remove(self.notification_list, index)
      self.write_fails = 0
      return request
    else
      self:shutdown_if_needed()
      return
    end
  end
end

---Default timeout policy for one sent request's response window (#10657).
---A request is emitted at most once per server generation; expiry is a
---terminal disposition, never a re-emission. Initialize keeps the longer
---policy the protocol reality needs (cold servers, large workspaces) while
---still never transmitting the same initialize frame twice under id 1.
Server.DEFAULT_REQUEST_TIMEOUT = 30
Server.INITIALIZE_REQUEST_TIMEOUT = 120

---One injectable clock input for request send/deadline decisions (#10657,
---clock-authority comment). The default stays wall-compatible with the
---#11103 fake-time harness; Lite XL integrations and tests can supply a
---monotonic source so wall-clock jumps cannot resend, prematurely expire,
---or indefinitely retain an operation.
function Server:now()
  return os.time()
end

---Sends due queued client requests exactly once each (#10657). The first
---successful write of an operation is its only emission; when its response
---window expires the correlation is removed exactly once through one typed
---"timeout" disposition (timeout_callback receives the request and the
---disposition), and a later server response for that id is stale - it finds
---no correlation and cannot run the original callback. Transport write
---failures are not semantic events: the unsent operation keeps its identity
---for a later tick while recovery/teardown remains owned by send_data and
---the process lifecycle.
function Server:process_requests()
  if not self.proc then return end

  local expired_index = nil
  local expired_request = nil

  for index, request in ipairs(self.request_list) do
    -- Queued operations enter with timestamp 0 (immediately due); sent
    -- operations come due at their response deadline; a transport failure
    -- re-paces the same unsent operation one tick later (#10657 keeps the
    -- upstream transport-retry pacing).
    if request.timestamp <= self:now() then
      -- only process when initialized or the initialize request
      -- which should be the first one.
      if not self.initialized and request.method ~= "initialize" then
        return nil
      end

      if request.times_sent == 0 then
        local message = {
          jsonrpc = '2.0',
          id = request.id,
          method = request.method,
          params = request.params or {}
        }

        local data = json.encode(message)

        local written, errmsg = self:write_request(data)

        if self.verbose then
          if written then
            self:log(
              "Sent request '%s':\n%s",
              request.method,
              util.jsonprettify(data)
            )
          else
            self:log(
              "Failed sending request '%s' with error: %s\n%s",
              request.method,
              errmsg or "unknown",
              util.jsonprettify(data)
            )
          end
        end

        if written then
          local time = request.timeout
            or (
              request.method == "initialize"
              and Server.INITIALIZE_REQUEST_TIMEOUT
            )
            or Server.DEFAULT_REQUEST_TIMEOUT
          request.timestamp = self:now() + time

          self.write_fails = 0

          -- Single-send (#10657): mark emitted; this operation will never
          -- be re-encoded or rewritten onto the wire again.
          request.times_sent = 1

          return request
        else
          request.timestamp = self:now() + 1
          self:shutdown_if_needed()
          return nil
        end
      else
        -- Deadline reached without a terminal response (#10657): remove
        -- the correlation exactly once and hand back one typed timeout
        -- disposition. No manufactured result, no server teardown, no
        -- second frame with this id.
        expired_index = index
        expired_request = request
        break
      end
    end
  end

  if expired_index then
    table.remove(self.request_list, expired_index)
    self:log(
      "Request '%s' timed out after one send (id %s)",
      tostring(expired_request.method),
      tostring(expired_request.id)
    )
    if expired_request.timeout_callback then
      expired_request.timeout_callback(expired_request, "timeout")
    end
  end

  return nil
end

---Read the lsp server stdout, parse any responses, requests or
---notifications and properly dispatch signals to any listeners.
function Server:process_responses()
  if not self.proc then return end

  local responses = self:read_responses(0)

  if type(responses) == "table" then
    for _, response in pairs(responses) do
      if self.verbose then
        self:log(
          "Processing Response:\n%s",
          util.jsonprettify(json.encode(response))
        )
      end
      if not response.id then
        -- A notification, event or generic message was received
        self:send_message_signal(response)
      elseif
        response.result
        or
        (not response.params and not response.method)
      then
        -- An actual request response was received
        self:send_response_signal(response)
      else
        -- The server is making a request
        self:send_request_signal(response)
      end
    end
  end

  return responses
end

---Sends all queued client responses to server.
function Server:process_client_responses()
  if not self.initialized then return end

  ::send_responses::
  for index, response in ipairs(self.response_list) do
    local message = {
      jsonrpc = '2.0',
      id = response.id
    }

    -- Explicit representation (#10785): admission already decided this
    -- entry's branch; the send loop encodes exactly that member instead of
    -- re-applying Lua truthiness (a queued result:false must reach the
    -- wire as result:false, never as an error frame).
    if response.error ~= nil then
      message.error = response.error
    elseif response.result ~= nil then
      message.result = response.result
    else
      -- Unreachable through push_response admission (#10785): keep the
      -- obligation's frame valid rather than emitting a member-less reply.
      message.result = json.null
    end

    local data = json.encode(message)

    if self.verbose then
        self:log("Sending client response:\n%s", util.jsonprettify(data))
    end

    local written, errmsg = self:write_request(data)

    if self.verbose then
      if not written then
        self:log(
          "Failed sending client response '%s' with error: %s",
          response.id,
          errmsg or "unknown"
        )
      end
    end

    if written then
      self.write_fails = 0
      table.remove(self.response_list, index)
      -- restart loop after removing from table to prevent issues
      goto send_responses
    else
      self:shutdown_if_needed()
      return
    end
  end
end

---Should be called periodically to prevent the server from stalling
---because of not flushing the stderr (especially true of clangd).
---@param log_errors boolean
function Server:process_errors(log_errors)
  if not self.proc then return end

  local errors = self:read_errors(0)

  if #errors > 0 and log_errors then
    -- Bounded stderr diagnostic (#11155): a category excerpt with visible
    -- truncation; full retention is capped inside read_errors.
    self:log("Server stderr: %s", bound_text(errors, 256))
  end

  return errors
end

---Sends raw data to the server process and ensures that all of it is written
---if no errors occur, otherwise it returns false and the error message. Notice
---that this function can perform yielding when ran inside of a coroutine.
---@param data string
---@return boolean sent
---@return string? errmsg
function Server:send_data(data)
  local proc = self.proc -- save current process to avoid it changing
  if not proc then return false end

  local failures, data_len = 0, #data
  local written, errmsg = proc:write(data)
  local total_written = written or 0

  while total_written < data_len and not errmsg do
    written, errmsg = proc:write(data:sub(total_written + 1))
    total_written = total_written + (written or 0)

    if (not written or written <= 0) and not errmsg and coroutine.running() then
      -- with each consecutive fail the yield timeout is increased by 5ms
      coroutine.yield((failures * 5) / 1000)

      failures = failures + 1
      if failures > 19 then -- after ~1000ms we error out
        errmsg = "maximum amount of consecutive failures reached"
        break
      end
    else
      failures = 0
    end
  end

  if errmsg then
    -- Bounded outbound diagnostic (#11155): direction, byte count, content
    -- digest and transport error only, never frame or body content.
    self:log(
      "Outbound write failure: bytes=%d digest=%s error=%s",
      data_len,
      content_digest(data),
      bound_text(errmsg)
    )
  end

  return total_written == data_len, errmsg
end

---Send one of the queued chunks of raw data to lsp server which are
---usually huge, like the textDocument/didOpen notification.
function Server:process_raw()
  if not self.initialized then return end

  -- Wait until everything else is processed to prevent initialization issues
  if
    #self.notification_list > 0
    or
    #self.request_list > 0
    or
    #self.response_list > 0
  then
    return
  end

  if not self.proc or not self.proc:running() then
    self.raw_list = {}
    return
  end

  local sent = false
  for index, raw in ipairs(self.raw_list) do
    raw.sending = true

    -- first send the header
    if
      not self:send_data(string.format(
        'Content-Length: %d\r\n\r\n', #raw.raw_data
      ))
    then
      break
    end

    if self.verbose then
      self:log("Raw header written")
    end

    -- send content in chunks
    local chunks = 10 * 1024
    raw.raw_data = raw.raw_data

    while #raw.raw_data > 0 do
      if not self.proc or not self.proc:running() then
        self.raw_list = {}
        return
      end

      if #raw.raw_data > chunks then
        -- TODO: perform proper error handling
        self:send_data(raw.raw_data:sub(1, chunks))
        raw.raw_data = raw.raw_data:sub(chunks+1)
      else
        -- TODO: perform proper error handling
        self:send_data(raw.raw_data)
        raw.raw_data = ""
      end

      self.write_fails = 0

      coroutine.yield()
    end

    if self.verbose then
      self:log("Raw content written")
    end

    if raw.callback then
      raw.callback(self, raw)
    end

    table.remove(self.raw_list, index)
    sent = true
    break
  end
  if sent then collectgarbage("collect") end
end

-- Local patch (#10833): rate control moved from enqueue admission to send
-- scheduling. The previous hit-rate dropper silently discarded load-bearing
-- traffic at enqueue time - didChange/watched-files/provider requests, and
-- even client responses owed to server requests - so document truth could go
-- stale while the client behaved as if every message was delivered. The
-- queues are drained one message per editor tick (responses flush fully),
-- which is the pacing mechanism; admission now only coalesces or explicitly
-- rejects with typed dispositions:
--
--   "queued"      stored for sending on a later tick
--   "coalesced"   replaced an unsent same-method entry in place (overwrite
--                 policy; the newer state supersedes the older frame)
--   "not_queued"  explicit typed rejection (server not initialized, or the
--                 request queue is full without an overwrite target); the
--                 optional not_queued_callback receives the reason and the
--                 method, and no phantom request id is allocated

---Queue a new notification. State-bearing notifications are always admitted;
---an overwrite-capable call coalesces onto its latest unsent same-method
---entry instead of growing the queue.
---@param method string
---@param options lsp.server.requestoptions
---@return string disposition One of "queued" or "coalesced"
function Server:push_notification(method, options)
  assert(options.params, "please provide the parameters for the notification")

  if options.overwrite then
    for _, notification in ipairs(self.notification_list) do
      if notification.method == method and not notification.sending then
        if self.verbose then
          self:log("Overwriting notification %s", tostring(method))
        end
        notification.params = options.params
        notification.callback = options.callback
        notification.data = options.data
        return "coalesced"
      end
    end
  end

  if self.verbose then
    self:log(
      "Pushing notification '%s':\n%s",
      method,
      util.jsonprettify(json.encode(options.params))
    )
  end

  -- Store the notification for later processing on responses_loop
  table.insert(self.notification_list, {
    method = method,
    params = options.params,
    callback = options.callback,
    data = options.data,
  })
  return "queued"
end

---Queue a new client request for paced sending. Requests are queued,
---coalesced under their declared overwrite policy, or explicitly rejected
---with a typed not_queued disposition - never silently dropped (#10833).
---Once sent, #10657 owns id/timeout correlation; queued work keeps its exact
---operation identity and no phantom ids are allocated for rejections.
---@param method string
---@param options lsp.server.requestoptions
---@return string disposition One of "queued", "coalesced" or "not_queued"
function Server:push_request(method, options)
  if not self.initialized and method ~= "initialize" then
    if options.not_queued_callback then
      options.not_queued_callback("not_initialized", method)
    end
    return "not_queued"
  end

  assert(options.params, "please provide the parameters for the request")

  -- Locate the declared overwrite policy target before any rejection path.
  -- An already-sent request is only marked overwritten once its replacement
  -- is actually admitted: marking first and rejecting second would suppress
  -- the in-flight request's valid response callback and lose both outcomes
  -- (#10833).
  local sent_overwrite_target = nil
  if options.overwrite then
    for _, request in ipairs(self.request_list) do
      if request.method == method then
        if request.times_sent > 0 then
          sent_overwrite_target = request
          break
        else
          request.params = options.params
          request.callback = options.callback
          request.overwritten_callback = options.overwritten_callback
          request.data = options.data
          request.timeout = options.timeout
          request.timeout_callback = options.timeout_callback
          request.timestamp = 0
          if self.verbose then
            self:log("Overwriting request %s", tostring(method))
          end
          return "coalesced"
        end
      end
    end
  end

  -- Bound with an explicit typed rejection (#10833): when the queue is full
  -- and this call has no unsent same-method overwrite target, the caller
  -- learns the operation was not queued instead of losing it silently.
  if #self.request_list >= self.max_queued_requests then
    if self.verbose then
      self:log("Request queue bound reached; not queueing %s", tostring(method))
    end
    if options.not_queued_callback then
      options.not_queued_callback("queue_full", method)
    end
    return "not_queued"
  end

  if self.verbose then
    self:log("Adding request %s", tostring(method))
  end

  -- Set the request id only after every rejection path is closed, so
  -- rejected operations leave no phantom in-flight identity.
  self.current_request = self.current_request + 1

  -- Store the request for later processing on responses_loop
  table.insert(self.request_list, {
    id = self.current_request,
    method = method,
    params = options.params,
    callback = options.callback,
    overwritten_callback = options.overwritten_callback,
    data = options.data,
    timeout = options.timeout,
    timeout_callback = options.timeout_callback,
    timestamp = 0,
    times_sent = 0
  })

  -- Replacement admitted: only now retire the already-sent predecessor so a
  -- rejected replacement can never strand the in-flight original (#10833).
  if sent_overwrite_target then
    sent_overwrite_target.overwritten = true
    if self.verbose then
      self:log("Overwriting request %s", tostring(method))
    end
  end

  return "queued"
end

---Queue a client response to a server request which can be an error
---or a regular response, one of both. A response is an obligation toward
---the server (#10785): it is always admitted and never dropped by any rate
---mechanism (#10833).
---
---Local patch (#10785): the result-vs-error branch is explicit, never Lua
---truthiness. A non-nil `error` argument selects the error branch and must
---be a real JSON-RPC error object - one missing code/message is wrapped into
---one typed internal error so an invalid payload can never silently produce
---a member-less or malformed frame. Otherwise ANY result value travels
---verbatim, including boolean false, 0, "", json.null and empty arrays and
---objects. Exactly one terminal response is admitted per accepted
---server-request occurrence: a duplicate handler attempt is rejected with a
---typed disposition instead of emitting a second frame, and the answered
---marker persists until the server admits a genuinely new request with that
---id or the generation tears down (#10785 review), so a late asynchronous
---handler replay can never double-send.
---@param method string
---@param id any JSON-RPC request id, echoed verbatim (number or string)
---@param result any Result payload; falsey values keep their identity
---@param error table|nil JSON-RPC error object; presence selects the branch
---@return string disposition One of "queued" or "rejected_duplicate"
function Server:push_response(method, id, result, error)
  if self.verbose then
    self:log("Adding response %s to %s", tostring(id), tostring(method))
  end

  -- Exactly one terminal response per accepted server request (#10785):
  -- a second handler attempt for an already-answered id is rejected
  -- instead of double-sending.
  if id ~= nil and self.answered_response_ids[id] then
    self:log(
      "Duplicate client response for server request id %s rejected",
      tostring(id)
    )
    return "rejected_duplicate"
  end

  -- Store the response for later processing on loop. Admission decides the
  -- branch exactly once so the send loop cannot re-apply truthiness.
  local response = {
    id = id
  }
  if error ~= nil then
    if
      type(error) ~= "table"
      or type(error.code) ~= "number"
      or type(error.message) ~= "string"
    then
      self:log(
        "Invalid client error payload for '%s' (%s); answering one typed internal error",
        tostring(method),
        type(error) ~= "table"
          and ("error value was " .. type(error))
          or (
            type(error.code) ~= "number"
            and "error code is not a number"
            or "error message is not a string"
          )
      )
      error = {
        code = Server.error_code.InternalError,
        message = "Internal error: invalid client error payload",
      }
    end
    response.error = error
  else
    response.result = result
  end

  if id ~= nil then
    self.pending_response_ids[id] = nil
    self.answered_response_ids[id] = true
  end

  table.insert(self.response_list, response)
  return "queued"
end

---Send raw json strings to server in cases where the json encoder
---would be too slow to convert a lua table into a json representation.
---@param name string A name to identify the request when overwriting.
---@param options lsp.server.requestoptions
function Server:push_raw(name, options)
  assert(options.raw_data, "please provide the raw_data for request")

  if options.overwrite then
    for _, request in ipairs(self.raw_list) do
      if request.method == name then
        if not request.sending then
          request.raw_data = options.raw_data
          request.callback = options.callback
          request.data = options.data
          if self.verbose then
            self:log("Overwriting raw request %s", tostring(name))
          end
          return
        end
        break
      end
    end
  end

  if self.verbose then
    self:log("Adding raw request %s", name)
  end

  -- Store the request for later processing on responses_loop
  table.insert(self.raw_list, {
    method = name,
    raw_data = options.raw_data,
    callback = options.callback,
    data = options.data,
  })
end

---Retrieve a request and removes it from the internal requests list
---@param id integer
---@return lsp.server.request | nil
function Server:pop_request(id)
  for index, request in ipairs(self.request_list) do
    if request.id == id then
      table.remove(self.request_list, index)
      return request
    end
  end
  return nil
end

---One typed terminal inbound framing failure (#11151).
---@class lsp.server.frame_failure
---@field kind string Always "lsp.frame_error".
---@field reason string Stable machine-readable failure token.
---@field header_bytes number|nil Header bytes observed when parsing failed.
---@field declared_length number|nil Declared Content-Length when one parsed.
---@field observed_body_bytes number|nil Body bytes accumulated on truncation.

---Tag carried by every typed inbound framing failure raised by the frame
---reader (#11151). Framing failures stay distinct from JSON syntax failures,
---request timeouts and process exit so consumers classify them exactly.
local FRAME_ERROR_KIND = "lsp.frame_error"

-- LuaJIT and Lua 5.1/5.2 do not provide math.tointeger (same runtime guard
-- as the staged json.lua numeric identity work). Doubles stay exact up to
-- 2^53, so integral values within that range keep working there; anything
-- larger fails closed exactly like the overflow class below.
local math_tointeger = math.tointeger
if not math_tointeger then
  math_tointeger = function(value)
    if value % 1 == 0 and value <= 9007199254740991 and value >= -9007199254740991 then
      return value
    end
    return nil
  end
end

---Builds one bounded framing-failure object. Only small numeric metadata is
---retained so hostile streams cannot inflate failure diagnostics, and no
---frame or body content is ever copied into the failure.
local function frame_failure(reason, meta)
  meta = meta or {}
  return {
    kind = FRAME_ERROR_KIND,
    reason = reason,
    header_bytes = meta.header_bytes,
    declared_length = meta.declared_length,
    observed_body_bytes = meta.observed_body_bytes
  }
end

---True when the given error object is a typed inbound framing failure.
local function is_frame_failure(error_object)
  return type(error_object) == "table"
    and error_object.kind == FRAME_ERROR_KIND
end

---Renders one bounded single-line diagnostic for a framing failure. Carries
---the reason and byte counts only; never frame or body content (#11151).
local function describe_frame_failure(failure)
  local parts = {
    "reason=" .. tostring(failure.reason),
    "header_bytes=" .. tostring(failure.header_bytes or 0)
  }
  if failure.declared_length ~= nil then
    parts[#parts + 1] = "declared=" .. tostring(failure.declared_length)
  end
  if failure.observed_body_bytes ~= nil then
    parts[#parts + 1] = "observed_body=" .. tostring(failure.observed_body_bytes)
  end
  return table.concat(parts, " ")
end

---Parses one LSP header block.
---
---Returns ok(boolean), declared_length(number)|nil, failure(table)|nil.
---Requires exactly one decimal Content-Length declaration: missing, signed,
---non-decimal, overflowing and duplicate declarations fail closed with
---distinct stable reasons (#11151). Other headers are ignored without being
---interpreted as length candidates.
local function parse_content_length(header_data)
  local declared = nil
  local position = 1
  while position <= #header_data do
    local line_end = header_data:find("\r\n", position, true)
    local line
    if line_end then
      line = header_data:sub(position, line_end - 1)
      position = line_end + 2
    else
      line = header_data:sub(position)
      position = #header_data + 1
    end

    local name, value = line:match("^%s*([^:%s]+)%s*:%s*(.-)%s*$")
    if name ~= nil and name:lower() == "content-length" then
      if value:match("^[+-]") then
        return false, nil, frame_failure("signed_content_length")
      elseif not value:match("^%d+$") then
        return false, nil, frame_failure("malformed_content_length")
      elseif declared ~= nil then
        -- Any second Content-Length declaration is rejected, including an
        -- identical one, so conflicting lengths can never be negotiated.
        return false, nil, frame_failure(
          "conflicting_content_length",
          { declared_length = declared }
        )
      else
        local numeric = tonumber(value)
        -- Decimal strings beyond the exact integer range cannot identify a
        -- realizable byte count and fail closed instead of approximating.
        if numeric == nil or math_tointeger(numeric) == nil then
          return false, nil, frame_failure("content_length_overflow")
        end
        declared = numeric
      end
    end
  end

  if declared == nil then
    return false, nil, frame_failure("missing_content_length")
  end

  return true, declared, nil
end

---Explicit bounded inbound frame parser state (#11151).
---
---Owns header accumulation, Content-Length validation, exact body
---consumption and remainder ownership across arbitrary process-read
---boundaries. Replaces hidden coroutine-local framing variables so parser
---state is deterministic and testable. All counts are Lua string lengths,
---never character or code-point counts, so multi-byte UTF-8 bodies survive
---any chunk splitting byte-exactly.
local FrameReader = {}
FrameReader.__index = FrameReader

---Creates one reader bounded by the given header/body budgets in bytes.
function FrameReader.new(max_header_bytes, max_body_bytes)
  return setmetatable({
    state = "headers",
    buffer = "",
    max_header_bytes = max_header_bytes,
    max_body_bytes = max_body_bytes,
    declared_length = nil,
    header_bytes = 0,
    failed = nil
  }, FrameReader)
end

---Feeds one chunk of process stdout into the parser.
---
---Returns a table { body = string } when exactly one complete frame finished,
---nil plus a typed failure when the stream violated the framing contract, or
---plain nil when more input is required. Bytes after a completed body stay
---buffered for the next frame.
function FrameReader:consume(chunk)
  if self.failed then
    return nil, self.failed
  end
  self.buffer = self.buffer .. chunk

  while true do
    if self.state == "headers" then
      local delimiter = self.buffer:find("\r\n\r\n", 1, true)
      if not delimiter then
        if #self.buffer > self.max_header_bytes then
          self.failed = frame_failure(
            "header_budget_exceeded",
            { header_bytes = #self.buffer }
          )
          return nil, self.failed
        end
        return nil
      end

      local header_data = self.buffer:sub(1, delimiter - 1)
      self.header_bytes = #header_data
      local ok, declared, failure = parse_content_length(header_data)
      if not ok then
        self.failed = failure
        return nil, failure
      end
      if declared > self.max_body_bytes then
        self.failed = frame_failure(
          "body_above_limit",
          { declared_length = declared }
        )
        return nil, self.failed
      end

      self.declared_length = declared
      self.state = "body"
      self.buffer = self.buffer:sub(delimiter + 4)

    elseif self.state == "body" then
      if #self.buffer < self.declared_length then
        return nil
      end

      local body = self.buffer:sub(1, self.declared_length)
      self.buffer = self.buffer:sub(self.declared_length + 1)
      self.declared_length = nil
      self.state = "headers"

      if #body == 0 then
        -- Reviewed disposition (#11151): a zero-length body cannot masquerade
        -- as no message nor reach JSON decoding, it fails closed.
        self.failed = frame_failure("empty_body")
        return nil, self.failed
      end

      return { body = body }
    end
  end
end

---Classifies end-of-stream against pending parser state.
---
---Returns clean(boolean), failure(table)|nil. A clean EOF carries zero
---pending bytes and simply ends the reader; EOF with a partial header or an
---incomplete body is a typed truncation failure carrying bounded offsets.
function FrameReader:eof()
  if self.failed then
    return false, self.failed
  end
  if self.state == "body" then
    return false, frame_failure(
      "truncated_body",
      {
        declared_length = self.declared_length,
        observed_body_bytes = #self.buffer
      }
    )
  elseif #self.buffer > 0 then
    return false, frame_failure(
      "truncated_header",
      { header_bytes = #self.buffer }
    )
  end
  return true, nil
end

---Try to fetch a server responses, notifications or requests
---in a specific amount of time.
---@param timeout integer Time in seconds, set to 0 to not wait
---@return table[]|boolean Responses list or false if failed
function Server:read_responses(timeout)
  local proc = self.proc -- save current process to avoid it changing
  if not proc or not proc:running() then
    return false
  end

  if not self.read_responses_coroutine then
    local max_header_bytes = self.max_header_bytes or Server.MAX_HEADER_BYTES
    local max_body_bytes = self.max_body_bytes or Server.MAX_BODY_BYTES
    self.read_responses_coroutine = coroutine.create(function()
      local reader = FrameReader.new(max_header_bytes, max_body_bytes)
      while true do
        -- First drain any complete frame already buffered, so an EOF arriving
        -- behind queued frames delivers them instead of truncating them.
        local result, failure = reader:consume("")
        if failure then
          return error(failure)
        elseif result then
          coroutine.yield(#reader.buffer > 0, result.body)
        else
          local buf = proc:read_stdout(Server.BUFFER_SIZE)

          if not buf then
            -- EOF / child death: a clean stop carries no pending bytes, any
            -- partial header or body is one typed truncation failure.
            local clean, eof_failure = reader:eof()
            if clean then
              return
            end
            return error(eof_failure)
          end

          result, failure = reader:consume(buf)
          if failure then
            return error(failure)
          elseif result then
            coroutine.yield(#reader.buffer > 0, result.body)
          else
            -- Input absorbed without completing a frame: end this turn. The
            -- next turn resumes here, mirroring upstream's read/yield rhythm
            -- and keeping stalled streams from spinning a turn forever.
            coroutine.yield(false)
          end
        end
      end
    end)
  end

  if coroutine.status(self.read_responses_coroutine) == "dead" then
    self.fatal_error = true
    self:shutdown_if_needed()
    return false
  end

  timeout = timeout or Server.DEFAULT_TIMEOUT
  local max_time = timeout == 0 and math.huge or system.get_time() + timeout

  local responses = {}
  repeat
    local status, has_more_data, response = coroutine.resume(self.read_responses_coroutine)
    if response then table.insert(responses, response) end
    if not status then
      local error_object = has_more_data
      self.fatal_error = true
      self:shutdown_if_needed()
      if is_frame_failure(error_object) then
        -- Bounded framing diagnostic: reason and byte counts only (#11151).
        self:log("Inbound framing failure: %s", describe_frame_failure(error_object))
      else
        self:log("Disconnecting from server: %s", bound_text(tostring(error_object)))
      end
      return false
    end
  until not has_more_data or (timeout > 0 and system.get_time() >= max_time)

  if #responses > 0 then
    for index, data in ipairs(responses) do
      -- Typed codec contract (#11197): failure is nil plus one typed decode
      -- error table; valid JSON false/null never take the nil shape.
      local json_data, decode_error = json.decode(data)
      if json_data ~= nil then
        responses[index] = json_data
      else
        responses[index] = nil
        -- Bounded decode diagnostic (#11155): codec reason, decode offset,
        -- byte count and content digest; the body itself is never echoed.
        self:log(
          "JSON decode failure: reason=%s offset=%s bytes=%d digest=%s",
          type(decode_error) == "table" and tostring(decode_error.reason) or "unknown",
          type(decode_error) == "table" and tostring(decode_error.byte_offset) or "?",
          #data,
          content_digest(data)
        )
        return false
      end
    end

    if #responses > 0 then
      -- Reset write fails since server is sending responses
      self.write_fails = 0

      return responses
    end
  elseif self.verbose and timeout > 0 then
    self:log("Could not read a response in %d seconds", timeout)
  end

  return false
end

---Get messages thrown by the stderr pipe of the server.
---@param timeout integer Time in seconds, set to 0 to not wait
---@return string|nil
function Server:read_errors(timeout)
  local proc = self.proc -- save current process to avoid it changing
  if not proc then return "" end

  timeout = timeout or Server.DEFAULT_TIMEOUT
  local inside_coroutine = self.yield_on_reads and coroutine.running() or false

  local max_time = os.time() + timeout
  if timeout == 0 then max_time = max_time + 1 end
  local output = ""
  while max_time > os.time() and output == "" do
    output = proc:read_stderr(Server.BUFFER_SIZE)
    if timeout == 0 then break end
    if output == "" and inside_coroutine then
      coroutine.yield()
    end
  end

  -- Drain stderr fully so the child can never block on a full pipe, but
  -- retain only a bounded window (#11155); discarded bytes are marked.
  local truncated = false
  if timeout == 0 and output ~= "" then
    local new_output = nil
    while new_output ~= "" do
      new_output = proc:read_stderr(Server.BUFFER_SIZE)
      if new_output ~= "" then
        if new_output == nil then
          break
        end

        if #output < Server.MAX_STDERR_BYTES then
          local keep = math.min(#new_output, Server.MAX_STDERR_BYTES - #output)
          output = output .. new_output:sub(1, keep)
          if keep < #new_output then
            truncated = true
          end
        else
          truncated = true
        end

        if inside_coroutine then
          coroutine.yield()
        end
      end
    end
    if truncated then
      output = output .. BOUNDED_TEXT_MARKER
    end
  end

  return output or ""
end

---Try to send a request to a server in a specific amount of time.
---@param data table | string Table or string with the json request
---@return boolean written
---@return string? errmsg
function Server:write_request(data)
  if not self.proc or not self.proc:running() then
    return false
  end

  if type(data) == "table" then
    data = json.encode(data)
  end

  -- WARNING: send_data performs yielding which can pontentially cause a
  -- race condition, in case of future issues this may be the root cause.
  return self:send_data(string.format(
    'Content-Length: %d\r\n\r\n%s',
    #data,
    data
  ))
end

function Server:log(message, ...)
  print (string.format("%s: " .. message .. "\n", self.name, ...))
end

---Call an apropriate signal handler for a given response.
---@param response table
function Server:send_response_signal(response)
  local request = self:pop_request(response.id)
  if request then
    if not request.overwritten and request.callback then
      request.callback(self, response, request)
    elseif request.overwritten and request.overwritten_callback then
      request.overwritten_callback(self, response, request)
    end
    return
  end
  self:on_response(response, request)
end

---Called for each response that doesn't has a signal handler.
---@param response table
---@param request lsp.server.request | nil
function Server:on_response(response, request)
  if self.verbose then
    self:log(
      "Received response '%s' with result:\n%s",
      response.id,
      util.jsonprettify(json.encode(response))
    )
  end
end

---Register a request handler.
---@param method string
---@param callback lsp.server.responsecb
function Server:add_request_listener(method, callback)
  if self.verbose then
    self:log(
      "Registering listener for '%s' requests",
      method
    )
  end

  if not self.request_listeners[method] then
    self.request_listeners[method] = {}
  end
  table.insert(self.request_listeners[method], callback)
end

---Call an apropriate signal handler for a given request.
---@param request table
function Server:send_request_signal(request)
  if not request.method then
    if self.verbose and request.id then
      self:log(
        "Received empty response for previous request '%s'",
        request.id
      )
    end
    return
  end

  -- Accepted server request (#10785): register its pending terminal-reply
  -- obligation so teardown can dispose of unanswered ids explicitly and a
  -- replacement generation starts clean. A genuinely new request occurrence
  -- with a reused id supersedes the previous occurrence's correlation
  -- (#10785 review); a late handler replay for the OLD occurrence stays
  -- rejected instead of emitting a second terminal frame.
  if request.id ~= nil then
    self.pending_response_ids[request.id] = true
    self.answered_response_ids[request.id] = nil
  end

  if self.request_listeners[request.method] then
    for _, l in ipairs(self.request_listeners[request.method]) do
      l(self, request)
    end
  else
    self:on_request(request)
  end
end

---Called for each request that doesn't has a signal handler.
---@param request table
function Server:on_request(request)
  if self.verbose then
    self:log(
      "Received request '%s' with data:\n%s",
      request.method,
      util.jsonprettify(json.encode(request))
    )
  end

  self:push_response(
    request.method,
    request.id,
    nil,
    {
      code = Server.error_code.MethodNotFound,
      message = "Method not found"
    }
  )
end

---Register a specialized message or notification listener.
---Notice that if no specialized listener is registered the
---on_notification() method will be called instead.
---@param method string
---@param callback lsp.server.notificationcb
function Server:add_message_listener(method, callback)
  if self.verbose then
    self:log(
      "Registering listener for '%s' messages",
      method
    )
  end

  if not self.message_listeners[method] then
    self.message_listeners[method] = {}
  end
  table.insert(self.message_listeners[method], callback)
end

---Call an apropriate signal handler for a given message or notification.
---@param message table
function Server:send_message_signal(message)
  if self.message_listeners[message.method] then
    for _, l in ipairs(self.message_listeners[message.method]) do
      l(self, message.params)
    end
  else
    self:on_message(message.method, message.params)
  end
end

---Called for every message or notification without a signal handler.
---@param method string
---@Param params table
function Server:on_message(method, params)
  if self.verbose then
    self:log(
      "Received notification '%s' with params:\n%s",
      method,
      util.jsonprettify(json.encode(params))
    )
  end
end

---Return the languageId for the specified doc.
---@param doc core.doc
---@return string
function Server:get_language_id(doc)
  if type(self.language) == "string" then
    return self.language
  else
    for _, l in ipairs(self.language) do
      if string.match(doc.abs_filename, l.pattern) then
        return l.id
      end
    end
  end
  return util.file_extension(doc.filename)
end

---Kills the server process and deinitialize the server object state.
function Server:stop()
  self.initialized = false
  self.proc = nil

  -- Explicit obligation teardown (#10785): unsent queued responses and
  -- unanswered accepted-request obligations die with this generation so
  -- old ids cannot leak into, or be satisfied by, a replacement server.
  self.request_list = {}
  self.response_list = {}
  self.notification_list = {}
  self.raw_list = {}
  self.pending_response_ids = {}
  self.answered_response_ids = {}
end

---Shutdown the server if not running or amount of write fails
---reached the maximum allowed.
function Server:shutdown_if_needed()
  if
    self.write_fails >= self.write_fails_before_shutdown
    or
    (self.proc and not self.proc:running())
    or
    self.fatal_error
  then
    self:stop()
    self:on_shutdown()
    return
  end
  self.write_fails = self.write_fails + 1
end

---Can be overwritten to handle server shutdowns.
function Server:on_shutdown()
  self:log("The server was shutdown.")
end

---Sends a shutdown notification to lsp and then stop it.
function Server:exit()
  self.initialized = false

  -- Send shutdown request
  local message = {
    jsonrpc = '2.0',
    id = self.current_request + 1,
    method = "shutdown",
    params = {}
  }

  self:write_request(json.encode(message))

  -- send exit notification
  self:notify('exit')

  self:stop()
end


return Server
