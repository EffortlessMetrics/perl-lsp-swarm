import {
  captureProviderRequestSubject,
  isProviderRequestSubjectCurrent,
} from '../providerRequestSubject';

describe('provider request subject', () => {
  test('detects document version changes after capture', () => {
    const document = {
      uri: { toString: () => 'file:///tmp/example.pl' },
      version: 3,
    };
    const subject = captureProviderRequestSubject(document as never);
    expect(isProviderRequestSubjectCurrent(subject, document as never)).toBe(true);

    const edited = {
      uri: { toString: () => 'file:///tmp/example.pl' },
      version: 4,
    };
    expect(isProviderRequestSubjectCurrent(subject, edited as never)).toBe(false);
  });
});
