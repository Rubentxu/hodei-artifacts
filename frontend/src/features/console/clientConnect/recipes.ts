/**
 * Native client connection recipes for console repositories.
 *
 * Recipes reuse the verified routes, client versions, and verification
 * status declared in `docs/community/ecosystems.md` (HSP-243) and resolve
 * their base URL strictly from a validated authority (HSP-238).
 *
 * Security invariants:
 * - Never embed hardcoded tokens or secrets (always reference env vars).
 * - Never disable TLS, checksum, or signature verification.
 * - Never read or trust unvalidated forwarded headers.
 */

import type {
  RepositoryFormat,
  RepositoryMode,
  RepositorySummary,
} from '../api/types';

export type VerificationStatus = 'VERIFIED' | 'NOT_RUN';

export type BaseUrlSource = 'configured' | 'browser';

export interface ValidatedOrigin {
  baseUrl: string;
  authority: string;
}

export interface ValidatedBaseUrl extends ValidatedOrigin {
  source: BaseUrlSource;
}

export interface ClientRecipeStep {
  title: string;
  command: string;
}

export interface ClientRecipe {
  format: RepositoryFormat;
  mode: RepositoryMode;
  clientLabel: string;
  clientVersion: string;
  verificationStatus: VerificationStatus;
  endpointUrl: string;
  credentialGuidance: string;
  readOnly: boolean;
  steps: ClientRecipeStep[];
}

const MAX_AUTHORITY_LENGTH = 255;

const hasControlOrWhitespace = (value: string): boolean => {
  if (/\s/.test(value)) {
    return true;
  }
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code < 0x20 || code === 0x7f) {
      return true;
    }
  }
  return false;
};

/**
 * Validates that a candidate origin is a clean `http` or `https` authority
 * without credentials, path, query, fragment, or control characters.
 */
export const validateOrigin = (
  raw: string | null | undefined
): ValidatedOrigin | null => {
  if (typeof raw !== 'string') {
    return null;
  }
  const trimmed = raw.trim();
  if (!trimmed || hasControlOrWhitespace(trimmed)) {
    return null;
  }

  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return null;
  }

  const protocol = parsed.protocol.toLowerCase();
  if (protocol !== 'http:' && protocol !== 'https:') {
    return null;
  }
  if (parsed.username !== '' || parsed.password !== '') {
    return null;
  }
  if (parsed.pathname !== '/' && parsed.pathname !== '') {
    return null;
  }
  if (parsed.search !== '' || parsed.hash !== '') {
    return null;
  }
  if (!parsed.host || parsed.host.length > MAX_AUTHORITY_LENGTH) {
    return null;
  }

  return {
    baseUrl: `${protocol}//${parsed.host}`,
    authority: parsed.host,
  };
};

/**
 * Resolves the base URL from a configured public URL first, falling back to
 * `window.location.origin` only when it passes authority validation.
 */
export const resolveValidatedBaseUrl = (
  configuredPublicUrl?: string | null,
  browserOrigin?: string | null
): ValidatedBaseUrl | null => {
  const configured = validateOrigin(configuredPublicUrl);
  if (configured) {
    return { ...configured, source: 'configured' };
  }

  const fallbackCandidate =
    browserOrigin !== undefined
      ? browserOrigin
      : typeof window !== 'undefined'
        ? window.location.origin
        : null;

  const browser = validateOrigin(fallbackCandidate);
  if (browser) {
    return { ...browser, source: 'browser' };
  }

  return null;
};

/**
 * Builds the contextual native client recipe for a repository's format and
 * modality using a validated base URL.
 */
export const buildClientRecipe = (
  repository: Pick<RepositorySummary, 'name' | 'format' | 'mode'>,
  rawBaseUrl: string
): ClientRecipe => {
  const origin = validateOrigin(rawBaseUrl);
  if (!origin) {
    throw new Error('Cannot build client recipe without a validated base URL');
  }

  const { baseUrl, authority } = origin;
  const repo = encodeURIComponent(repository.name);
  const { format, mode } = repository;
  const readOnly = mode !== 'hosted';

  switch (format) {
    case 'maven2': {
      const endpointUrl = `${baseUrl}/repository/${repo}`;
      const serverXml = [
        '<settings>',
        '  <servers>',
        '    <server>',
        `      <id>hodei-${repo}</id>`,
        '      <configuration>',
        '        <httpHeaders>',
        '          <httpHeader>',
        '            <name>Authorization</name>',
        '            <value>Bearer ${env.HODEI_TOKEN}</value>',
        '          </httpHeader>',
        '        </httpHeaders>',
        '      </configuration>',
        '    </server>',
        '  </servers>',
        '</settings>',
      ].join('\n');

      return {
        format,
        mode,
        clientLabel: 'Apache Maven',
        clientVersion: '3.9+',
        verificationStatus: 'VERIFIED',
        endpointUrl,
        credentialGuidance:
          'Store your token in ~/.m2/settings.xml (permissions 600) with Authorization: Bearer ${env.HODEI_TOKEN}. Never place tokens in URLs or CLI arguments.',
        readOnly,
        steps: [
          {
            title: 'Configure ~/.m2/settings.xml',
            command: serverXml,
          },
          readOnly
            ? {
                title: `Resolve dependencies (${mode}, read-only)`,
                command: `mvn dependency:resolve -DremoteRepositories=hodei-${repo}::default::${endpointUrl}`,
              }
            : {
                title: 'Publish or resolve artifacts (hosted)',
                command: `mvn deploy -DaltDeploymentRepository=hodei-${repo}::${endpointUrl}`,
              },
        ],
      };
    }

    case 'npm': {
      const endpointUrl = `${baseUrl}/npm/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'npm',
        clientVersion: '10+',
        verificationStatus: 'VERIFIED',
        endpointUrl,
        credentialGuidance: `Configure .npmrc with //${authority}/npm/${repo}/:_authToken=\${HODEI_TOKEN}. Keep TLS active in production.`,
        readOnly,
        steps: [
          {
            title: 'Configure .npmrc',
            command: [
              `//${authority}/npm/${repo}/:_authToken=\${HODEI_TOKEN}`,
              `registry=${endpointUrl}/`,
            ].join('\n'),
          },
          readOnly
            ? {
                title: `Install packages (${mode}, read-only)`,
                command: `npm install <package> --registry=${endpointUrl}/`,
              }
            : {
                title: 'Publish and install packages (hosted)',
                command: [
                  `npm publish --registry=${endpointUrl}/`,
                  `npm install <package> --registry=${endpointUrl}/`,
                ].join('\n'),
              },
        ],
      };
    }

    case 'pypi': {
      const endpointUrl = `${baseUrl}/pypi/${repo}/simple/`;
      const uploadUrl = `${baseUrl}/pypi/${repo}/`;
      return {
        format,
        mode,
        clientLabel: 'pip / twine',
        clientVersion: '24+ / 5+',
        verificationStatus: 'VERIFIED',
        endpointUrl,
        credentialGuidance:
          'Export TWINE_USERNAME=__token__ and TWINE_PASSWORD="${HODEI_TOKEN}" (or configure ~/.pypirc with mode 600) over HTTPS.',
        readOnly,
        steps: readOnly
          ? [
              {
                title: `Install packages (${mode}, read-only)`,
                command: `pip install --index-url "${endpointUrl}" <package>`,
              },
            ]
          : [
              {
                title: 'Upload distributions with twine (hosted)',
                command: `TWINE_USERNAME=__token__ TWINE_PASSWORD="\${HODEI_TOKEN}" twine upload --repository-url "${uploadUrl}" dist/*`,
              },
              {
                title: 'Install packages with pip (PEP 503)',
                command: `pip install --index-url "${endpointUrl}" <package>`,
              },
            ],
      };
    }

    case 'oci': {
      const endpointUrl = `${baseUrl}/v2/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'docker / podman / oras',
        clientVersion: '26+ / 5+ / 1.2+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance: `Authenticate with docker login ${authority} using your system credential helper over HTTPS.`,
        readOnly,
        steps: [
          {
            title: 'Authenticate registry',
            command: `docker login ${authority}`,
          },
          readOnly
            ? {
                title: `Pull OCI artifact (${mode}, read-only)`,
                command: `docker pull ${authority}/${repo}/<image>:<tag>`,
              }
            : {
                title: 'Push and pull OCI artifact (hosted)',
                command: [
                  `docker push ${authority}/${repo}/<image>:<tag>`,
                  `docker pull ${authority}/${repo}/<image>:<tag>`,
                ].join('\n'),
              },
        ],
      };
    }

    case 'raw': {
      const endpointUrl = `${baseUrl}/repository/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'curl',
        clientVersion: '8+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance:
          'Send Authorization: Bearer ${HODEI_TOKEN} in the request header over HTTPS.',
        readOnly,
        steps: [
          readOnly
            ? {
                title: `Download artifact (${mode}, read-only)`,
                command: `curl -fsSL -H "Authorization: Bearer \${HODEI_TOKEN}" "${endpointUrl}/<path>" -o <file>`,
              }
            : {
                title: 'Upload and download artifact (hosted)',
                command: [
                  `curl -fsSL -X PUT -H "Authorization: Bearer \${HODEI_TOKEN}" --data-binary @<file> "${endpointUrl}/<path>"`,
                  `curl -fsSL -H "Authorization: Bearer \${HODEI_TOKEN}" "${endpointUrl}/<path>" -o <file>`,
                ].join('\n'),
              },
        ],
      };
    }

    case 'cargo': {
      const endpointUrl = `sparse+${baseUrl}/cargo/${repo}/`;
      return {
        format,
        mode,
        clientLabel: 'cargo',
        clientVersion: '1.80+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance:
          'Store your token in $CARGO_HOME/credentials.toml (mode 600) or CARGO_REGISTRIES_HODEI_TOKEN.',
        readOnly,
        steps: [
          {
            title: 'Configure .cargo/config.toml',
            command: `[registries.hodei]\nindex = "${endpointUrl}"`,
          },
          {
            title: readOnly
              ? `Search crates (${mode})`
              : 'Publish crate (hosted)',
            command: readOnly
              ? 'cargo search --registry hodei <crate>'
              : 'cargo publish --registry hodei',
          },
        ],
      };
    }

    case 'nuget': {
      const endpointUrl = `${baseUrl}/nuget/${repo}/v3/index.json`;
      return {
        format,
        mode,
        clientLabel: '.NET SDK',
        clientVersion: '8.0+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance:
          'Pass --api-key "${HODEI_TOKEN}" via environment variable or keep credentials out of versioned nuget.config.',
        readOnly,
        steps: [
          {
            title: 'Register NuGet v3 source',
            command: `dotnet nuget add source "${endpointUrl}" --name "hodei-${repo}"`,
          },
          {
            title: readOnly
              ? `Restore packages (${mode})`
              : 'Push package (hosted)',
            command: readOnly
              ? `dotnet restore --source "hodei-${repo}"`
              : `dotnet nuget push <package>.nupkg --source "hodei-${repo}" --api-key "\${HODEI_TOKEN}"`,
          },
        ],
      };
    }

    case 'go': {
      const endpointUrl = `${baseUrl}/go/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'go',
        clientVersion: '1.22+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance: `Configure ~/.netrc (mode 600) for ${authority} over HTTPS.`,
        readOnly,
        steps: [
          {
            title: `Fetch module via GOPROXY (${mode})`,
            command: `GOPROXY="${endpointUrl}" go get <module>@<version>`,
          },
        ],
      };
    }

    case 'apt': {
      const endpointUrl = `${baseUrl}/apt/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'apt',
        clientVersion: '2.7+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance: `Install the public key from ${endpointUrl}/key.asc with signed-by and store credentials in /etc/apt/auth.conf.d/ (mode 600).`,
        readOnly,
        steps: [
          {
            title: 'Install repository signing key',
            command: `curl -fsSL "${endpointUrl}/key.asc" | sudo tee /etc/apt/keyrings/hodei-${repo}.asc > /dev/null`,
          },
        ],
      };
    }

    case 'rpm': {
      const endpointUrl = `${baseUrl}/yum/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'dnf / rpm',
        clientVersion: '4.14+ / 4.18+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance: `Keep gpgcheck=1 and repo_gpgcheck=1 with gpgkey=${endpointUrl}/key.asc.`,
        readOnly,
        steps: [
          {
            title: 'Configure DNF repository',
            command: `sudo dnf config-manager --add-repo "${endpointUrl}/"`,
          },
        ],
      };
    }

    case 'helm': {
      const endpointUrl = `${baseUrl}/v2/${repo}`;
      return {
        format,
        mode,
        clientLabel: 'helm',
        clientVersion: '3.14+',
        verificationStatus: 'NOT_RUN',
        endpointUrl,
        credentialGuidance: `Helm charts are served on the OCI surface (/v2/{repo}/...). Authenticate with helm registry login ${authority} over HTTPS.`,
        readOnly,
        steps: [
          {
            title: 'Authenticate and pull OCI chart',
            command: [
              `helm registry login ${authority}`,
              `helm pull oci://${authority}/${repo}/<chart> --version <version>`,
            ].join('\n'),
          },
        ],
      };
    }

    default:
      throw new Error(`No native client recipe is available for ${format}`);
  }
};
