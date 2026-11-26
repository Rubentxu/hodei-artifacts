/**
 * Display labels for known repository formats.
 *
 * Unknown formats returned by the server fall back to their wire name so
 * newly supported formats appear without updating a frontend allowlist.
 */
const FORMAT_LABELS: Record<string, string> = {
  raw: 'Raw',
  oci: 'OCI',
  maven2: 'Maven 2',
  npm: 'npm',
  pypi: 'PyPI',
  cargo: 'Cargo',
  nuget: 'NuGet',
  go: 'Go',
  helm: 'Helm',
  apt: 'APT',
  rpm: 'RPM',
};

export const formatLabel = (format: string): string =>
  FORMAT_LABELS[format] ?? format;
