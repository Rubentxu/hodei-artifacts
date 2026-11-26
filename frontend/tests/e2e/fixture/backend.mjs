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
import { randomBytes } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { theAssetPath, theContent, theDigest, theRepository } from './seed.mjs';
import {
  aTokenFor,
  subjectOf,
  theAudience,
  theIssuer,
  theSecret,
  theStudioPrincipals,
  theToken,
  theTokenPath,
  theTokenPathOf,
} from './token.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** The root of the repository, where the service and its bundle live. */
const repositoryRoot = resolve(here, '..', '..', '..', '..');

/**
 * Where the fixture works.
 *
 * It is under `target/`, which is not in the repository, and not in the
 * temporary directory of the system: a fixture that dies without being
 * asked to stop leaves its data behind, and the next run has to find it
 * and take it away instead of seeding next to it.
 */
const workingDirectory = join(repositoryRoot, 'target', 'e2e', 'fixture');

/** The port the service listens on. */
const port = Number(process.env.HODEI_FIXTURE_PORT ?? 18887);
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
  const target = process.env.CARGO_TARGET_DIR ?? theCargoTargetDirectory();
  return join(target, 'debug', 'hodei-artifacts-api');
};

/** The target directory that `cargo` would use. */
const theCargoTargetDirectory = () => {
  const result = spawnSync(
    'cargo',
    ['metadata', '--no-deps', '--format-version', '1'],
    {
      cwd: repositoryRoot,
      encoding: 'utf8',
    }
  );
  if (result.status !== 0) return join(repositoryRoot, 'target');
  return JSON.parse(result.stdout).target_directory;
};

/**
 * The Dgraph alpha the service runs on.
 *
 * The only profile of the service is the graph (`RUN_MODE=dgraph`), so the
 * fixture needs a REAL alpha, with the same convention as the graph
 * integration tests (`tests/support/graph_profile.rs`):
 *
 * - `HODEI_GRAPH_IT_ENDPOINT`: `host:port` of the gRPC listener (required).
 * - `HODEI_GRAPH_IT_USER` / `HODEI_GRAPH_IT_PASSWORD`: the guardian of the
 *   galaxy namespace, needed to create the namespace of the fixture (ACL is
 *   mandatory).
 * - `HODEI_GRAPH_IT_ADMIN`: `host:port` of the alpha HTTP port (default
 *   `<host of the endpoint>:8080`).
 *
 * The fixture creates its own ACL namespace and deletes it when it stops, so
 * it never sees (or leaves) data in the shared alpha.
 */
const theAlpha = () => {
  const endpoint = process.env.HODEI_GRAPH_IT_ENDPOINT;
  const user = process.env.HODEI_GRAPH_IT_USER;
  const password = process.env.HODEI_GRAPH_IT_PASSWORD;
  if (!endpoint || !user || !password) {
    throw new Error(
      'the fixture needs a Dgraph alpha: set HODEI_GRAPH_IT_ENDPOINT (host:port of the gRPC listener), HODEI_GRAPH_IT_USER and HODEI_GRAPH_IT_PASSWORD (the galaxy guardian), and optionally HODEI_GRAPH_IT_ADMIN (host:port of the HTTP port)'
    );
  }
  const host = endpoint.includes(':')
    ? endpoint.slice(0, endpoint.lastIndexOf(':'))
    : endpoint;
  const admin = process.env.HODEI_GRAPH_IT_ADMIN ?? `${host}:8080`;
  return { endpoint, user, password, admin };
};

/** One GraphQL call to the `/admin` of the alpha. */
const adminCall = async (alpha, query, token) => {
  const response = await fetch(`http://${alpha.admin}/admin`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      ...(token ? { 'X-Dgraph-AccessToken': token } : {}),
    },
    body: JSON.stringify({ query }),
  });
  const answer = await response.json();
  if (answer.errors) {
    throw new Error(
      `the alpha refused an admin call: ${JSON.stringify(answer.errors)}`
    );
  }
  return answer.data;
};

/** The guardian token of the galaxy namespace. */
const galaxyToken = async alpha => {
  const data = await adminCall(
    alpha,
    `mutation { login(userId: ${JSON.stringify(alpha.user)}, password: ${JSON.stringify(alpha.password)}, namespace: 0) { response { accessJWT } } }`
  );
  return data.login.response.accessJWT;
};

/** Creates the ACL namespace of this run, with a throwaway `groot`. */
const createNamespace = async alpha => {
  const password = randomBytes(18).toString('hex');
  const token = await galaxyToken(alpha);
  const data = await adminCall(
    alpha,
    `mutation { addNamespace(input: { password: ${JSON.stringify(password)} }) { namespaceId message } }`,
    token
  );
  return { id: data.addNamespace.namespaceId, password };
};

/** Deletes the namespace of this run (best effort: it is isolated garbage). */
const deleteNamespace = async (alpha, id) => {
  try {
    const token = await galaxyToken(alpha);
    await adminCall(
      alpha,
      `mutation { deleteNamespace(input: { namespaceId: ${id} }) { namespaceId } }`,
      token
    );
  } catch (error) {
    console.error(`the namespace ${id} could not be deleted: ${error.message}`);
  }
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

/** The routes of the Policy Studio that only read. */
const theStudioReads = [
  'get-policy-studio-head',
  'get-policy-studio-revisions',
  'get-policy-studio-history',
];

/**
 * The Cedar bundle of the lane.
 *
 * It is the example bundle plus what the Policy Studio needs to tell the
 * principals apart: `carol` is the approver (the admin grant), `viewer`
 * has a permit per route that only reads, and `bob` has none. The example
 * bundle is not edited: the lane writes its own copy next to the data it
 * seeds and points the service at it.
 */
const writeBundle = directory => {
  const examples = join(repositoryRoot, 'config', 'examples');
  const schema = join(directory, 'schema.json');
  const policies = join(directory, 'policies.cedar');
  copyFileSync(join(examples, 'schema.json'), schema);
  const grants = [
    `permit(principal == Iam::User::"${subjectOf('carol')}", action == Admin::Action::"AccessAdminApi", resource);`,
    ...theStudioReads.map(
      route =>
        `permit(principal == Iam::User::"${subjectOf('viewer')}", action == Admin::Action::"AccessAdminApi", resource == Admin::AdminApi::"hrn:hodei:admin:global:acme:AdminApi/${route}");`
    ),
  ];
  writeFileSync(
    policies,
    `${readFileSync(join(examples, 'policies.cedar'), 'utf8')}\n${grants.join('\n')}\n`
  );
  return { schema, policies };
};

/** The tokens of the principals of the studio, each in its own file. */
const writeStudioTokens = () => {
  for (const name of theStudioPrincipals) {
    writeFileSync(theTokenPathOf(name), aTokenFor(subjectOf(name)), {
      mode: 0o600,
    });
  }
};

const removeStudioTokens = () => {
  for (const name of theStudioPrincipals) {
    rmSync(theTokenPathOf(name), { force: true });
  }
};

const main = async () => {
  const binary = theBinary();
  if (!existsSync(binary)) {
    throw new Error(
      `the service is not built: ${binary} is not there. Build it with "cargo build --bin hodei-artifacts-api".`
    );
  }

  const directory = workingDirectory;
  rmSync(directory, { recursive: true, force: true });
  mkdirSync(directory, { recursive: true });
  mkdirSync(dirname(theTokenPath), { recursive: true });
  const bundle = writeBundle(directory);
  const alpha = theAlpha();
  const namespace = await createNamespace(alpha);
  const readiness = theReadiness();
  const service = spawn(binary, [], {
    cwd: repositoryRoot,
    env: {
      ...process.env,
      HODEI_SERVER__HOST: '127.0.0.1',
      HODEI_SERVER__PORT: String(port),
      // The graph profile: `config/dgraph.toml` declares `[graph]`, and the
      // alpha, the namespace and the credentials come from the environment.
      RUN_MODE: 'dgraph',
      HODEI_GRAPH__ENDPOINT: alpha.endpoint,
      HODEI_GRAPH__TLS__ENABLED: 'false',
      HODEI_GRAPH__TLS__CA_CERT_PATH: '',
      HODEI_GRAPH__TLS__SERVER_NAME: '',
      HODEI_GRAPH__NAMESPACE: String(namespace.id),
      HODEI_GRAPH__CREDENTIALS__USERID: 'groot',
      HODEI_GRAPH__CREDENTIALS__PASSWORD: namespace.password,
      HODEI_CONTENT__BACKEND: 'filesystem',
      // The catalog lives in the durable store: the console reads the
      // repositories, the assets and the decisions from there, and the
      // in-memory slice has none of them.
      HODEI_REPOSITORY__DURABLE: 'true',
      HODEI_CONTENT__CAS_PATH: join(directory, 'cas'),
      HODEI_SECURITY__ADMIN_API_ENABLED: 'true',
      HODEI_SECURITY__RAW_API_ENABLED: 'true',
      HODEI_SECURITY__AUTH_JWT_SECRET: theSecret,
      HODEI_SECURITY__AUTH_ISSUER: theIssuer,
      HODEI_SECURITY__AUTH_AUDIENCE: theAudience,
      HODEI_SECURITY__AUTHORIZATION_POLICY_PATH: bundle.policies,
      HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH: bundle.schema,
      HODEI_TRUST__POLICY_FILE: join(here, 'trust-policy.json'),
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  });

  const stop = async () => {
    service.kill('SIGTERM');
    readiness.server.close();
    rmSync(directory, { recursive: true, force: true });
    rmSync(theTokenPath, { force: true });
    removeStudioTokens();
    await deleteNamespace(alpha, namespace.id);
  };
  process.on('SIGTERM', () => {
    stop().finally(() => process.exit(0));
  });
  process.on('SIGINT', () => {
    stop().finally(() => process.exit(0));
  });
  // A fixture that dies without being asked to stop still takes its data
  // and its token with it.
  process.on('exit', () => {
    rmSync(directory, { recursive: true, force: true });
    rmSync(theTokenPath, { force: true });
    removeStudioTokens();
  });

  await waitFor(service);
  await seed();
  // The token is signed before the fixture says it is ready: a test that
  // reads a token that is not there yet is a test that fails for the
  // wrong reason.
  writeFileSync(theTokenPath, theToken(), { mode: 0o600 });
  writeStudioTokens();
  readiness.announce();
  console.log(`the fixture of the console is ready at ${address}`);
};

main().catch(error => {
  console.error(error);
  process.exit(1);
});
