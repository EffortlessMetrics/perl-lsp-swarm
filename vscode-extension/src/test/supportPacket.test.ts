import {
  type SupportPacketV1,
  formatSupportPacketHuman,
  serializeSupportPacketJson,
  supportAtom,
  supportDigest,
  supportKnown,
  supportState,
  validateSupportPacket,
} from '../supportPacket';

function packet(): SupportPacketV1 {
  return {
    schema_version: 'perl_lsp_support_packet.v1',
    product: {
      name: supportAtom('perl-lsp'),
      version: supportKnown(supportAtom('0.18.0')),
      track: supportKnown(supportAtom('public-beta')),
    },
    extension: {
      id: supportAtom('EffortlessMetrics.perl-lsp-rs'),
      version: supportKnown(supportAtom('0.18.0')),
      artifact_digest: supportKnown(
        supportDigest('sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'),
      ),
    },
    host: {
      editor: supportAtom('Visual Studio Code'),
      editor_version: supportKnown(supportAtom('1.125.1')),
      platform: supportAtom('linux'),
      architecture: supportAtom('x64'),
      extension_host: 'local',
      workspace_mode: 'multi_root',
      trust: 'trusted_supported',
    },
    perllsp: {
      state: 'known',
      role: 'managed',
      version: supportKnown(supportAtom('0.18.0')),
      target: supportKnown(supportAtom('x86_64-unknown-linux-gnu')),
      digest: supportKnown(
        supportDigest('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'),
      ),
      compatibility: 'ready_exact',
    },
    perl_dap: {
      state: 'known_absent',
      role: 'unknown',
      version: supportState('known_absent'),
      target: supportState('known_absent'),
      digest: supportState('known_absent'),
      compatibility: 'missing',
    },
    lifecycle: {
      generation: supportKnown(supportAtom('generation-7')),
      readiness: supportAtom('ready'),
      startup_disposition: supportAtom('started_exact'),
      crash_disposition: supportAtom('none'),
      activation_disposition: supportAtom('active'),
      managed_install_disposition: supportAtom('verified'),
    },
    protocol: {
      families: [
        {
          family: supportAtom('testing'),
          version: supportKnown(supportAtom('1')),
          state: 'ready_exact',
        },
        {
          family: supportAtom('readiness'),
          version: supportKnown(supportAtom('1')),
          state: 'ready_exact',
        },
      ],
    },
    configuration: {
      user_present: true,
      workspace_present: true,
      folder_present: false,
      project_config_present: true,
      formatter_mode: supportAtom('native'),
      critic_mode: supportAtom('native'),
      migration: {
        registry: supportKnown(supportAtom('vscode_configuration_migration.v1')),
        encountered: [supportAtom('v017_mcp_servers_removed')],
        status: 'inert',
      },
    },
    failure: {
      startup_reason: supportAtom('none'),
      network_reason: supportAtom('none'),
      provider_state: supportAtom('exact_current'),
      cache_state: supportAtom('stable'),
    },
  };
}

describe('support packet', () => {
  test('serializes deterministic bounded JSON and human projections', () => {
    const value = packet();
    expect(validateSupportPacket(value)).toEqual([]);

    const firstJson = serializeSupportPacketJson(value);
    const secondJson = serializeSupportPacketJson(value);
    expect(firstJson).toBe(secondJson);
    expect(firstJson.indexOf('readiness')).toBeLessThan(firstJson.indexOf('testing'));

    const human = formatSupportPacketHuman(value);
    expect(human).toContain('Support packet: perl_lsp_support_packet.v1');
    expect(human).toContain('perllsp: managed 0.18.0 ready_exact');
    expect(human).toContain('Migration: inert');
  });

  test.each([
    '/home/alice/private/project',
    'C:\\Users\\alice\\project',
    '\\\\server\\share',
    'https://user:secret@example.invalid/path',
    'TOKEN=secret',
    'line1\nline2',
  ])('rejects path, URL, secret-like, or multiline atoms: %s', (unsafe) => {
    expect(() => supportAtom(unsafe)).toThrow();
  });

  test('rejects non-sha256 digest material', () => {
    expect(() => supportDigest('abc123')).toThrow('support digest must be an exact sha256 digest');
  });

  test('keeps unknown and not-proven evidence explicit', () => {
    const value = packet();
    value.product.version = supportState('not_proven');
    value.host.editor_version = supportState('unknown');

    expect(validateSupportPacket(value)).toEqual([]);
    expect(formatSupportPacketHuman(value)).toContain('Product: perl-lsp not_proven');
    expect(formatSupportPacketHuman(value)).toContain('Editor: Visual Studio Code unknown');
  });

  test('rejects values attached to non-known evidence', () => {
    const value = packet();
    value.product.version = {
      state: 'unknown',
      value: supportAtom('0.18.0'),
    };

    expect(validateSupportPacket(value)).toContain(
      'product.version carries a value without known evidence',
    );
    expect(() => serializeSupportPacketJson(value)).toThrow('invalid support packet');
  });

  test('bounds protocol and migration lists', () => {
    const value = packet();
    value.protocol.families = Array.from({ length: 33 }, (_, index) => ({
      family: supportAtom(`family-${index}`),
      version: supportKnown(supportAtom('1')),
      state: 'ready_exact' as const,
    }));

    expect(validateSupportPacket(value)).toContain(
      'protocol family list exceeds bounded support-packet limit',
    );
  });
});
