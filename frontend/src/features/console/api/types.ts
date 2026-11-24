/**
 * The shapes that the API answers with.
 *
 * They are the shapes of the contract, and they keep the names that the
 * contract uses: a console that renames what the API says is a console
 * that disagrees with it, and the disagreement shows up in the bugs
 * nobody can reproduce.
 */

/**
 * A repository format identifier on the wire.
 *
 * The deployment capabilities endpoint is the source of truth for which
 * formats are served; the frontend does not maintain a closed allowlist.
 */
export type RepositoryFormat = string;

export type RepositoryMode = 'hosted' | 'proxy' | 'group';

export type ServeChannel = 'open' | 'trusted';

export type TrustState = 'quarantined' | 'trusted';

/** Where the bytes live: with everybody, or with the account. */
export type DigestScope = 'global' | 'account_partitioned';

export interface ContentDigest {
  algorithm: string;
  value: string;
}

/** A repository of the catalog, as the console sees it. */
export interface RepositorySummary {
  name: string;
  format: RepositoryFormat;
  mode: RepositoryMode;
  channel: ServeChannel;
}

/** The repositories of the account that the principal may read. */
export interface RepositoryList {
  repositories: RepositorySummary[];
}

/** What a format admits. */
export interface FormatCapability {
  name: RepositoryFormat;
  modes: string[];
}

/** What can be created. */
export interface Capabilities {
  formats: FormatCapability[];
}

/** An asset found by the search. */
export interface SearchHit {
  repository: string;
  format: RepositoryFormat;
  asset_path: string;
  package_name: string;
  package_version: string;
  digest: ContentDigest;
  size_bytes: number;
  trust_state: TrustState;
  updated_at: string;
}

/**
 * A page of the search.
 *
 * The total is `null` when it could not be counted: there are more
 * matches than the API counts, and it says that there are more without
 * saying how much. A total that was not counted is not a total.
 */
export interface SearchPage {
  total: number | null;
  more: boolean;
  limit: number;
  offset: number;
  results: SearchHit[];
}

/** Where the bytes are. */
export interface BlobView {
  digest: ContentDigest;
  scope: DigestScope;
  size_bytes: number;
  updated_at: string;
}

/** What was published, as it was published. */
export interface PublishedView {
  package_name: string;
  package_version: string;
  digest: ContentDigest;
  size_bytes: number;
  published_at: string;
}

/** A manifest that hangs from another one. */
export interface ReferrerView {
  asset_path: string;
  digest: ContentDigest;
  size_bytes: number;
  subject: ContentDigest;
}

/** The line of the manifests. */
export interface LineageView {
  subject: ContentDigest | null;
  referrers: ReferrerView[];
}

/** The decision that grants a content. */
export interface DecisionView {
  decision_id: string;
  actor: string;
  granted_at: string;
}

/** What has been decided about the content. */
export interface TrustView {
  state: TrustState;
  decision: DecisionView | null;
}

/**
 * One dimension of the trust profile.
 *
 * The state is a word and not a flag: "there is no evidence" is not
 * "the evidence is there and it holds", and a boolean cannot tell them
 * apart.
 */
export interface TrustDimension {
  name: string;
  state: string;
  evidence: unknown[];
  reason?: string;
}

/** The subject of a profile, as the HRN is serialized. */
export interface HrnView {
  partition: string;
  service: string;
  /** The region is not there when the resource is global. */
  region?: string;
  account_id: string;
  resource_type: string;
  resource_id: string;
}

/** The profile of the trust, dimension by dimension. */
export interface TrustProfile {
  subject: HrnView;
  dimensions: TrustDimension[];
}

/** What the evidence says about the content. */
export interface CapsuleView {
  subject: string;
  version: string;
  content_hash: string;
  profile: TrustProfile;
}

/** The detail of a package. */
export interface PackageView {
  repository: string;
  asset_path: string;
  package_name: string;
  package_version: string;
  digest: ContentDigest;
  size_bytes: number;
  updated_at: string;
  blob: BlobView;
  published: PublishedView;
  lineage: LineageView;
  capsule: CapsuleView;
  trust: TrustView;
}

/** The answer of a promotion. */
export interface PromotionResult {
  promoted: boolean;
  decision_id: string;
  digest: string;
  granted: string[];
  risk_version?: number;
}

/** The answer of a revocation. */
export interface RevokeResult {
  revoked: number;
}

/** The body of a creation of a repository. */
export interface CreateRepositoryBody {
  name: string;
  format: RepositoryFormat;
  mode: string;
  region?: string;
  settings?: Record<string, unknown>;
}

/** The class of an investigation finding. */
export type InvestigationFindingKind =
  | 'verified_fact'
  | 'evidence_backed_claim'
  | 'inference'
  | 'uncertainty'
  | 'recommendation';

/** The verification status of an investigation finding. */
export type InvestigationVerificationStatus =
  | 'verified'
  | 'unverified'
  | 'contradicted'
  | 'unverifiable';

/** A finding returned by an investigation report. */
export interface InvestigationFinding {
  kind: InvestigationFindingKind;
  statement: string;
  citations: string[];
  status: InvestigationVerificationStatus;
  basis: string;
}

/** How much of the available scope was included in the report. */
export type InvestigationCoverage =
  | 'complete'
  | { partial: { omitted: number } };

/** The kind of resource stamped in an investigation read-set. */
export type InvestigationReadStampKind = 'evidence' | 'relation' | 'policy';

/** A digest-verified stamp of a resource read during investigation. */
export interface InvestigationReadStamp {
  kind: InvestigationReadStampKind;
  resource: string;
  verified_by: string;
}

/** The manifest of sources and stamps read during investigation. */
export interface InvestigationReadSet {
  generations: Array<[string, number]>;
  stamps: InvestigationReadStamp[];
}

/** The request body for `POST /api/v1/investigations`. */
export interface InvestigationRequest {
  subject: string;
  question?: string;
  max_evidence?: number;
  max_relations?: number;
  max_bytes?: number;
}

/** The versioned report returned by `POST /api/v1/investigations`. */
export interface InvestigationReport {
  schema_version: string;
  subject: string;
  findings: InvestigationFinding[];
  coverage: InvestigationCoverage;
  read_set: InvestigationReadSet;
}
