/**
 * What the fixture puts in the catalog.
 *
 * The names live here and not in the script that starts the service,
 * because the tests have to name the same things that the service holds:
 * a repository, an asset inside it and the digest of its bytes.
 */

import { createHash } from 'node:crypto';

/** The repository the fixture creates. */
export const theRepository = 'console-fixture';

/** The path of the asset the fixture publishes. */
export const theAssetPath = 'packages/console-fixture-1.0.0.tar.gz';

/** The bytes of the asset the fixture publishes. */
export const theContent = Buffer.from(
  'hodei artifacts console fixture\n',
  'utf8'
);

/** The digest of the content, as the service names it. */
export const theDigest = `sha256:${createHash('sha256')
  .update(theContent)
  .digest('hex')}`;
