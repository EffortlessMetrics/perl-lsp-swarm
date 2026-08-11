# Perl Debugging Support

Perl debugging is available via the `perl-dap` Debug Adapter Protocol (DAP) server. The current CLI uses the native adapter (direct `perl -d`) with basic stepping and breakpoints.

## Current Status (Native Adapter)

- **Launch debugging**: supported
- **TCP attach**: transport handshake is present; interactive attach remains unsupported because ordinary control and inspection requests are not routed through attached sessions yet
- **Variables/evaluate**: parsed values are used when available; placeholder fallback remains when parsing provides no value, and scorecard receipts are a separate verification gap
- **Legacy bridge**: compatibility-only; native launch uses the built-in runtime; TCP attach remains a transport-only preview

## Features

### Core Debugging (Native Adapter)
- **Breakpoints**: Set breakpoints in your Perl code (best-effort)
- **Step Controls**: Step over, step into, step out
- **Call Stack**: Navigate through the call stack (best-effort)
- **Variable Inspection**: Parsed native-adapter values are rendered when available; placeholder fallback remains for unavailable values
- **Evaluate**: Native-adapter evaluation is available for launched sessions where the active session routes the request
- **Conditional Breakpoints**: Best-effort conditions via Perl debugger

### Test Debugging
- Debug individual test functions
- Debug entire test files
- Integrated with Test Explorer
- TAP output support during debugging

## Installation

### 1. Install the Debug Adapter
```bash
# Build and install the debug adapter
cargo install --path crates/perl-dap
```

### 2. Configure VSCode
The Perl Language Server extension automatically detects and uses the debug adapter.

## Usage

### Quick Start
1. Open a Perl file in VSCode
2. Set breakpoints by clicking in the gutter
3. Press F5 or use Run → Start Debugging
4. Select "Perl: Launch Script" configuration

### Debug Configurations

#### Basic Script Debugging
```json
{
    "type": "perl",
    "request": "launch",
    "name": "Launch Perl Script",
    "program": "${file}",
    "stopOnEntry": true,
    "args": []
}
```

#### Test File Debugging
```json
{
    "type": "perl",
    "request": "launch",
    "name": "Debug Perl Test",
    "program": "${file}",
    "stopOnEntry": false,
    "args": [],
    "env": {
        "PERL_TEST_HARNESS_DUMP_TAP": "1"
    }
}
```

#### Custom Working Directory
```json
{
    "type": "perl",
    "request": "launch",
    "name": "Launch with Custom CWD",
    "program": "${file}",
    "cwd": "${workspaceFolder}/scripts",
    "args": ["--verbose"]
}
```

### Debugging from Test Explorer
1. Open the Testing panel in VSCode
2. Navigate to a test
3. Right-click and select "Debug Test"
4. Or use the debug icon next to the test

## Debug Commands

### Execution Control
- **Continue** (F5): Resume execution
- **Step Over** (F10): Execute current line
- **Step Into** (F11): Step into subroutines
- **Step Out** (Shift+F11): Step out of current subroutine
- **Restart** (Ctrl+Shift+F5): Restart debugging session
- **Stop** (Shift+F5): Stop debugging

### Breakpoints
- Click in the gutter to toggle breakpoints
- Right-click for conditional breakpoints
- Use the Breakpoints panel to manage all breakpoints

### Variables
- Native launch sessions render debugger values in the Variables panel. Attached sessions remain transport-only until ordinary control and inspection requests are routed.
- Session correctness and evaluate scorecards are tracked separately in [DAP status](../project/status/dap.md).

## Configuration Options

### launch.json Properties

| Property | Type | Description | Default |
|----------|------|-------------|---------|
| `program` | string | Path to Perl script | `${file}` |
| `args` | array | Command line arguments | `[]` |
| `stopOnEntry` | boolean | Stop at first line | `false` |
| `cwd` | string | Working directory | `${workspaceFolder}` |
| `env` | object | Environment variables | `{}` |
| `perlPath` | string | Path to Perl interpreter | `perl` |

`launch` is supported by the native adapter. TCP attach has a transport handshake, but ordinary control and evaluation requests are not routed yet; do not treat it as a supported interactive mode.

## Troubleshooting

### Workspace Build Issues

#### "Cannot find parser.c" or "libclang not found"
The old v0.8.8 workspace report is historical. The current workspace excludes legacy `tree-sitter-perl`, `fuzz`, and `archive`; use the current workspace commands below rather than treating that release report as a live contract:

```bash
# ✅ This should work (workspace tests only production crates)
cargo test

# ⚠️ The legacy top-level tree-sitter-perl component is excluded; the maintained C compatibility crate is tree-sitter-perl-c
cargo build -p tree-sitter-perl-c
```

**Solution**: The workspace is configured to exclude problematic crates. Use the standard workspace commands:

```bash
# Build only production crates
cargo build

# Test only production crates  
cargo test

# Check workspace configuration
cargo check
```

#### Feature Conflicts Between Crates
If you see feature resolution errors:

```bash
# ✅ Use workspace-level commands
cargo test

# ❌ Avoid direct crate builds that may conflict
cargo test -p example-crate-with-conflicts
```

**Reference**: See the [workspace status report](../project/status/workspace.md) for current workspace status.

### Debug adapter not found
```bash
# Verify installation
which perl-dap

# Reinstall if needed
cargo install --path crates/perl-dap --force
```

### Breakpoints not working
1. Ensure the file is saved
2. Check that perl-dap is running
3. Verify Perl syntax is correct

### Variables not showing
- Parsed values are shown when the native adapter returns them; placeholder values remain when parsing returns no value.
- Scorecard coverage for session correctness and evaluate remains incomplete.

## Architecture

The debugging system consists of:

1. **Debug Adapter (perl-dap)**: Native DAP adapter (default CLI); launch is supported and TCP attach is transport-only
2. **Legacy bridge compatibility**: Kept separate from the native launch and TCP attach path
3. **Perl Debugger Integration**: Interfaces with `perl -d`
4. **VSCode Extension**: Provides UI integration
5. **Test Integration**: Connects with Test Explorer

## Limitations

- Remote debugging not yet supported
- TCP attach remains a transport-only preview; session scorecards remain a distinct verification surface
- Some variable/evaluate edge cases may not be covered by the current scorecard receipts
- Some Perl internals may not be inspectable

## Future Enhancements

- [ ] Remote debugging support
- [ ] Attach to running Perl process
- [ ] Data structure visualization
- [ ] Performance profiling integration
- [ ] Multi-threaded debugging support
