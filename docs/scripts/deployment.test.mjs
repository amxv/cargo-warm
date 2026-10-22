import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const docs = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.dirname(docs);

test('Vercel never creates quota-consuming Git-triggered deployments', () => {
  const config = JSON.parse(readFileSync(path.join(docs, 'vercel.json'), 'utf8'));
  assert.equal(config.git.deploymentEnabled, false);
  assert.equal(Object.hasOwn(config, 'ignoreCommand'), false);
  assert.equal(config.outputDirectory, 'dist');
});

test('only docs changes on main request a production deployment', () => {
  const workflow = readFileSync(path.join(repo, '.github/workflows/docs.yml'), 'utf8');
  for (const event of ['push', 'pull_request']) {
    const section = workflow.match(new RegExp(`^  ${event}:\\n([\\s\\S]*?)(?=^  (?:push|pull_request|workflow_dispatch):|^permissions:)`, 'm'))?.[1];
    assert.ok(section, `missing ${event} trigger`);
    assert.match(section, /branches: \[main\]/);
    const paths = [...section.matchAll(/^      - "([^"]+)"$/gm)].map(match => match[1]);
    assert.deepEqual(paths, ['docs/**']);
  }
  assert.match(workflow, /^  workflow_dispatch:$/m);
  assert.match(workflow, /github\.ref == 'refs\/heads\/main'/);
  assert.match(workflow, /secrets\.VERCEL_TOKEN/);
  assert.match(workflow, /vars\.VERCEL_ORG_ID/);
  assert.match(workflow, /vars\.VERCEL_PROJECT_ID/);
  assert.match(workflow, /bun run build/);
  assert.match(workflow, /node scripts\/prepare-prebuilt\.mjs/);
  assert.match(workflow, /working-directory: \./);
  assert.match(workflow, /mv docs\/\.vercel\/output \.vercel\/output/);
  assert.match(workflow, /vercel@latest deploy --prebuilt --prod/);
});
