/**
 * The credential of the fixture.
 *
 * The token is signed with the shared secret that the service reads at
 * boot, and it says the same things a real token says: who the principal
 * is, which account it belongs to, who issued it and who it is for. The
 * secret lives in the fixture and in the environment of the service, and
 * it never reaches the command line of a process that another one can
 * read.
 *
 * The secret is made when the fixture starts, and it is not written in
 * the repository: a secret that is in the repository is a secret that
 * anybody with the repository can sign a token with, and this one signs
 * an administrator of the example account. What the tests read is the
 * token that the fixture already signed, from the file the fixture
 * leaves.
 */

import { createHmac, randomBytes } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The root of the repository, where the service and its bundle live. */
const repositoryRoot = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  '..',
  '..',
  '..'
);

/** The secret the service verifies with. */
export const theSecret = randomBytes(32).toString('hex');

/** Who issues the token. */
export const theIssuer = 'https://issuer.example';

/** Who the token is for. */
export const theAudience = 'hodei-artifacts';

/** The principal of the fixture. */
export const theSubject = 'hrn:hodei:iam::acme:User/alice';

/** The account of the fixture. */
export const theAccount = 'acme';

/** The base64url of a value. */
const base64url = value => Buffer.from(value).toString('base64url');

/**
 * A token for the principal of the fixture.
 *
 * The signature is computed over the header and the payload with the
 * shared secret, which is what the service verifies.
 */
export const theToken = (expiresInSeconds = 3600) => {
  const header = base64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const now = Math.floor(Date.now() / 1000);
  const payload = base64url(
    JSON.stringify({
      sub: theSubject,
      account: theAccount,
      iss: theIssuer,
      aud: theAudience,
      iat: now,
      exp: now + expiresInSeconds,
    })
  );
  const signature = createHmac('sha256', theSecret)
    .update(`${header}.${payload}`)
    .digest('base64url');
  return `${header}.${payload}.${signature}`;
};

/**
 * Where the fixture leaves the token it signed.
 *
 * The tests read the token from here, and not from a signer of their own:
 * the secret is in the fixture, and the token is what the tests need.
 */
export const theTokenPath = resolve(
  repositoryRoot,
  'target',
  'e2e',
  'token.txt'
);
