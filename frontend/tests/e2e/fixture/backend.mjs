/**
 * The service of the console, in fixture mode.
 *
 * It is the real service: the same binary, the same Cedar bundle and the
 * same catalog that a deployment runs. What the fixture adds is the data
 * and a credential that may read it. The console that the end-to-end
 * test drives talks to the service that ships, and not to a stand-in
 * that answers what the test expects.
 *
 * The data is seeded **through the API**, with the same calls a person
 * would make: a repository is created, an asset is published through the
 * content surface and the asset is promoted. A fixture that wrote to the
 * database directly would prove that the console can read a table, and
 * not that it can read the service.
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  theAssetPath,
  theContent,
  theDigest,
  theRepository,
} from './seed.mjs';
import { theAudience, theIssuer, theSecret, theToken } from './token.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** The root of the repository, where the service and its bundle live. */
const repositoryRoot = resolve(here, '..', '..', '..', '..');

/** The port the service listens on. */
const port = Number(process.env.HODEI_FIXTURE_PORT ?? 18787);
/** The address the service listens on. */
const address = `http://127.0.0.1:${port}`;
/**
 * The port the fixture announces its readiness on.
 *
 * The service answers `/health` as soon as it is up, and that is before
 * the catalog has been seeded. A test that starts against a half-seeded
 * catalog measures the seeding and not the console, so the readiness is
 * announced here, when the seeding is done.
 */
const readyPort = port + 1;

/**
 * The binary of the service.
 *
 * It is looked for where `cargo` puts it, and `cargo` is asked, because
 * the target directory is a property of the machine and not of the
 * repository.
 */
const theBinary = () => {
  const explicit = process.env.HODEI_API_BIN;
  if (explicit) return explicit;
  const target =
    process.env.CARGO_TARGET_DIR ?? theCargoTargetDirectory();
  return join(target, 'debug', 'hodei-artifacts-api');
};

/** The target directory that `cargo` would use. */
const theCargoTargetDirectory = () => {
  const result = spawnSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  });
  if (result.status !== 0) return join(repositoryRoot, 'target');
  return JSON.parse(result.stdout).target_directory;
};

/** The answer of a request to the service. */
const ask = async (path, options = {}) => {
  const response = await fetch(`${address}${path}`, {
    ...options,
    headers: {
      authorization: `Bearer ${theToken()}`,
      ...(options.headers ?? {}),
    },
  });
  const body = await response.text();
  return { status: response.status, body };
};

/** Waits until the service answers. */
const waitFor = async service => {
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    if (service.exitCode !== null) {
      throw new Error(
        `the service stopped before it answered: the exit code is ${service.exitCode}`
      );
    }
    try {
      const response = await fetch(`${address}/health`);
      if (response.ok) return;
    } catch {
      // The service is not listening yet.
    }
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  throw new Error('the service did not answer');
};

/**
 * Seeds the catalog through the API.
 *
 * The calls are the ones the console makes, and the ones a person makes:
 * create the repository, publish the asset, promote it.
 */
const seed = async () => {
  const created = await ask('/api/v1/catalog/repositories', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      name: theRepository,
      format: 'raw',
      mode: 'hosted',
      region: 'eu',
    }),
  });
  if (created.status !== 201 && created.status !== 409) {
    throw new Error(
      `the repository could not be created: ${created.status} ${created.body}`
    );
  }

  const published = await ask(`/repository/${theRepository}/${theAssetPath}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/octet-stream' },
    body: theContent,
  });
  if (published.status !== 201 && published.status !== 200) {
    throw new Error(
      `the asset could not be published: ${published.status} ${published.body}`
    );
  }

  const promoted = await ask('/api/v1/promotions', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      repository: theRepository,
      path: theAssetPath,
      digest: theDigest,
    }),
  });
  if (promoted.status !== 200 && promoted.status !== 201) {
    throw new Error(
      `the asset could not be promoted: ${promoted.status} ${promoted.body}`
    );
  }
};

/**
 * The server that announces that the fixture is ready.
 *
 * It answers 200 when the catalog is seeded and 503 while it is not, so
 * that whoever waits for it waits for the data and not only for the
 * process.
 */
const theReadiness = () => {
  let ready = false;
  const server = createServer((request, response) => {
    response.writeHead(ready ? 200 : 503, {
      'content-type': 'text/plain',
    });
    response.end(ready ? 'ready' : 'the catalog is not seeded yet');
  });
  server.listen(readyPort, '127.0.0.1');
  return {
    server,
    announce: () => {
      ready = true;
    },
  };
};

const main = async () => {
  const binary = theBinary();
  if (!existsSync(binary)) {
    throw new Error(
      `the service is not built: ${binary} is not there. Build it with "cargo build --bin hodei-artifacts-api".`
    );
  }

  const directory = mkdtempSync(join(tmpdir(), 'hodei-console-fixture-'));
  const readiness = theReadiness();
  const service = spawn(binary, [], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      HODEI_SERVER__HOST: '127.0.0.1',
      HODEI_SERVER__PORT: String(port),
      HODEI_DATABASE__DB_TYPE: 'rocksdb',
      HODEI_DATABASE__NAMESPACE: 'hodei',
      HODEI_DATABASE__DATABASE: 'artifacts',
      HODEI_SCHEMA__STORAGE_TYPE: 'rocksdb',
      // The catalog lives in the durable store: the console reads the
      // repositories, the assets and the decisions from there, and the
      // in-memory slice has none of them.
      HODEI_REPOSITORY__DURABLE: 'true',
      HODEI_ROCKSDB__PATH: join(directory, 'catalog.rocksdb'),
      HODEI_CONTENT__CAS_PATH: join(directory, 'cas'),
      HODEI_SECURITY__ADMIN_API_ENABLED: 'true',
      HODEI_SECURITY__RAW_API_ENABLED: 'true',
      HODEI_SECURITY__AUTH_JWT_SECRET: theSecret,
      HODEI_SECURITY__AUTH_ISSUER: theIssuer,
      HODEI_SECURITY__AUTH_AUDIENCE: theAudience,
      HODEI_SECURITY__AUTHORIZATION_POLICY_PATH: join(
        repositoryRoot,
        'config',
        'examples',
        'policies.cedar'
      ),
      HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH: join(
        repositoryRoot,
        'config',
        'examples',
        'schema.json'
      ),
      HODEI_TRUST__POLICY_FILE: join(here, 'trust-policy.json'),
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  });

  const stop = () => {
    service.kill('SIGTERM');
    readiness.server.close();
    rmSync(directory, { recursive: true, force: true });
  };
  process.on('SIGTERM', () => {
    stop();
    process.exit(0);
  });
  process.on('SIGINT', () => {
    stop();
    process.exit(0);
  });

  await waitFor(service);
  await seed();
  readiness.announce();
  console.log(`the fixture of the console is ready at ${address}`);
};

main().catch(error => {
  console.error(error);
  process.exit(1);
});
