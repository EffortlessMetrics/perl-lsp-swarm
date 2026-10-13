const test = require('node:test');
const assert = require('node:assert/strict');
const yaml = require('js-yaml');

const EMPTY_MERGE_FIXTURE = 'arr: &arr [{}, {}, {}, {}]\n' + 'target:\n' + '  <<: *arr\n';
const NONEMPTY_MERGE_FIXTURE = 'base: &base {answer: 42}\n' + 'target:\n' + '  <<: *base\n';

function mergeError(source, maxTotalMergeKeys) {
  assert.throws(
    () =>
      yaml.load(source, {
        schema: yaml.YAML11_SCHEMA,
        maxTotalMergeKeys,
      }),
    (error) => {
      if (!(error instanceof yaml.YAMLException)) {
        return false;
      }
      const typedError = /** @type {{ reason?: string }} */ (error);
      return typedError.reason?.includes('maxTotalMergeKeys') === true;
    },
  );
}

void test('counts empty merge sources against the configured merge budget', () => {
  const parsed = yaml.load(EMPTY_MERGE_FIXTURE, {
    schema: yaml.YAML11_SCHEMA,
    maxTotalMergeKeys: 4,
  });
  assert.deepEqual(parsed.target, {});
  mergeError(EMPTY_MERGE_FIXTURE, 3);
});

void test('preserves valid nonempty merges at their exact budget', () => {
  const parsed = yaml.load(NONEMPTY_MERGE_FIXTURE, {
    schema: yaml.YAML11_SCHEMA,
    maxTotalMergeKeys: 2,
  });
  assert.equal(parsed.target.answer, 42);
  mergeError(NONEMPTY_MERGE_FIXTURE, 1);
});
