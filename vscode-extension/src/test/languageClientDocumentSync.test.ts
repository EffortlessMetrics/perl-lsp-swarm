import { replayOpenPerlDocuments } from '../languageClientDocumentSync';

describe('replayOpenPerlDocuments', () => {
  test('replays current Perl buffers and ignores other languages', async () => {
    const sendNotification = jest.fn().mockResolvedValue(undefined);

    await replayOpenPerlDocuments({ sendNotification }, [
      {
        uri: 'file:///workspace/probe.pl',
        languageId: 'perl',
        version: 7,
        text: 'my $value = 7;\n',
      },
      {
        uri: 'file:///workspace/readme.md',
        languageId: 'markdown',
        version: 2,
        text: '# ignored\n',
      },
    ]);

    expect(sendNotification).toHaveBeenCalledTimes(1);
    expect(sendNotification).toHaveBeenCalledWith('textDocument/didOpen', {
      textDocument: {
        uri: 'file:///workspace/probe.pl',
        languageId: 'perl',
        version: 7,
        text: 'my $value = 7;\n',
      },
    });
  });

  test('preserves notification failures for restart callers', async () => {
    const failure = new Error('client stopped');
    const sendNotification = jest.fn().mockRejectedValue(failure);

    await expect(
      replayOpenPerlDocuments({ sendNotification }, [
        { uri: 'file:///workspace/probe.pl', languageId: 'perl', version: 1, text: '1;\n' },
      ]),
    ).rejects.toBe(failure);
  });
});
