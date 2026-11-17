/**
 * The credential of the fixture.
 *
 * The token is signed with the shared secret that the service reads at
 * boot, and it says the same things a real token says: who the principal
 * is, which account it belongs to, who issued it and who it is for. The
 * secret lives in the fixture and in the environment of the service, and
 * it never reaches the command line of a process that another one can
 * read.
 */

import { createHmac } from 'node:crypto';

/** The secret the service verifies with. */
export const theSecret = 'console-fixture-secret-long-enough';

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
