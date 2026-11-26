/**
 * The shapes the Policy Studio reads and writes.
 *
 * They mirror the wire contract of `/api/v1/policy-studio` (the handlers
 * in `src/handlers/policy_studio.rs`) one field at a time. What the
 * backend does not answer with is not here, and what is not here cannot
 * be asked for: there is no shadow mode, no choice of editor or approver
 * and no account. The editor and the approver are the principal of the
 * token, and the account is the one the server verifies.
 */

/** The dimensions of the engine that a requirement may name. */
export const DIMENSIONS = [
  'integrity',
  'source_assurance',
  'build_provenance',
  'builder_trust',
  'inventory_completeness',
  'signature_state',
  'evidence_freshness',
  'test_evidence',
  'vulnerability_risk',
  'security_evidence',
  'reachable_risk',
  'runtime_exposure',
  'compliance_state',
  'active_exceptions',
  'policy_state',
] as const;

/** What happens when a requirement is not met. */
export type OnFailure = 'deny' | 'require_approval';

/** The kinds of requirement the engine speaks (`RequirementKind`). */
export const REQUIREMENT_KINDS = [
  'dimension',
  'no_reachable_critical_vulnerability',
  'risk_established',
  'exposure_at_most',
] as const;

/** What a requirement asks for. */
export type RequirementKind =
  | { kind: 'dimension'; dimension: string; accepts: string[] }
  | { kind: 'no_reachable_critical_vulnerability' }
  | { kind: 'risk_established' }
  | { kind: 'exposure_at_most'; exposure: string };

/** One requirement of a policy. */
export interface Requirement {
  kind: RequirementKind;
  on_failure: OnFailure;
  overridable_by: string | null;
}

/** A trust policy, as the engine reads it. */
export interface StudioPolicy {
  id: string;
  version: string;
  schema_version: string;
  requirements: Requirement[];
}

/** The head of a policy set: the revision that decides. */
export interface Head {
  policy_id: string;
  sequence: number;
  digest: string;
  activated_by: string;
  editor: string;
  reason: string;
  activated_at: string;
}

/** `GET /policy-studio/head`. */
export interface HeadResponse {
  exists: boolean;
  head?: Head;
}

/** `GET /policy-studio/revisions`. */
export interface RevisionResponse {
  policy_id: string;
  sequence: number;
  digest: string;
  policy: StudioPolicy;
  saved_by: string;
  reason: string;
  active: boolean;
}

/** A revision as the lifecycle stores it (history and save). */
export interface StoredRevision {
  policy_id: string;
  sequence: number;
  digest: string;
  policy: StudioPolicy;
  saved_by: string;
  saved_at: string;
  reason: string;
  /** Present when the revision re-saves a target: who rolled back. */
  rolled_back_by?: string;
}

/** `POST /policy-studio/validate`. */
export interface ValidateResponse {
  valid: boolean;
  error?: string;
  applied: boolean;
}

/** `POST /policy-studio/revisions`. */
export interface SaveResponse {
  revision: StoredRevision;
  applied: boolean;
}

/** The kind of one subject's answer: three things never to confuse. */
export type EffectKind = 'POLICY_DECISION' | 'EVIDENCE_GAP' | 'ENGINE_ERROR';

/** How a decision of the engine came out. */
export type Outcome =
  | 'ALLOW'
  | 'DENY'
  | 'WARN'
  | 'REQUIRE_APPROVAL'
  | 'INDETERMINATE';

/** The hole an evidence gap names. */
export interface EvidenceGap {
  requirement: string;
  dimension: string | null;
  reason: string;
  evidence_ids: string[];
}

/** The part of a trust decision the studio shows. */
export interface TrustDecisionView {
  outcome: Outcome;
  reasons?: string[];
  policy_id: string;
  policy_version: string;
  evidence_ids: string[];
}

/** What the engine said about one subject. */
export interface SimulatedEffect {
  subject: string;
  kind: EffectKind;
  decision: TrustDecisionView | null;
  gap: EvidenceGap | null;
  error: string | null;
}

/** The change of one subject between the active policy and the candidate. */
export interface EffectChange {
  subject: string;
  before: Outcome;
  after: Outcome;
  loosens: boolean;
}

/** `POST /policy-studio/simulate`. */
export interface SimulateResponse {
  candidate: { policy: StudioPolicy; revision: number | null };
  active_version?: string;
  effects: SimulatedEffect[];
  changes: EffectChange[];
  applied: boolean;
}

/** `POST /policy-studio/activate` and `/rollback`. */
export interface ActivationResponse {
  head: Head;
  applied: boolean;
}

/** One audit event of the lifecycle. */
export interface HistoryEvent {
  id: string;
  kind: string;
  policy_id: string;
  sequence: number;
  digest: string;
  expected_head: number | null;
  new_head: number;
  actor: string;
  editor: string;
  reason: string;
  at: string;
  rolled_back_by?: string;
}

/**
 * `GET /policy-studio/history`.
 *
 * The window is bounded: `*_truncated` says that there is more after it.
 * The order is the one of the store: revisions by sequence, oldest first,
 * and events in the order they happened.
 */
export interface HistoryResponse {
  offset: number;
  limit: number;
  revisions_truncated: boolean;
  events_truncated: boolean;
  policy_id: string;
  revisions: StoredRevision[];
  events: HistoryEvent[];
}

/** An evidence snapshot to measure the candidate against. */
export interface SimulatedSubject {
  subject: string;
  context: {
    version: string;
    subject: { hrn: string; digest: string | null };
    target: 'staging' | 'production';
    dimensions: { name: string; state: string }[];
    capsule_hash: string | null;
    risk: {
      critical_known: number;
      critical_reachable: number;
      exposure: 'INTERNAL' | 'NETWORK' | 'INTERNET' | 'UNKNOWN';
      vex_accepted: number;
      established: boolean;
      risk_version: number;
    };
    exceptions: unknown[];
    evidence_ids: string[];
    assembled_at: string;
  };
}

/** The error body of the studio (`StudioError`). */
export interface StudioApiError {
  error: string;
  message: string;
  /** Only on `stale_head`: the head the store actually has. */
  found_head?: number;
}

/** The body of the studio's error, when it is one. */
export const studioErrorOf = (data: unknown): StudioApiError | null => {
  if (typeof data !== 'object' || data === null) return null;
  const body = data as Partial<StudioApiError>;
  if (typeof body.error !== 'string' || typeof body.message !== 'string') {
    return null;
  }
  return {
    error: body.error,
    message: body.message,
    found_head:
      typeof body.found_head === 'number' ? body.found_head : undefined,
  };
};

/** The page size of the history: the server clamps it to 1..=200. */
export const HISTORY_PAGE_SIZE = 20;

/**
 * Whether every requirement of the policy is of a kind the engine speaks.
 * A policy that carries another kind is shown but not edited: offering a
 * way to edit what the studio does not understand would be offering a
 * capability that does not exist.
 */
export const isStudioEditable = (policy: StudioPolicy): boolean =>
  Array.isArray(policy.requirements) &&
  policy.requirements.every(requirement =>
    (REQUIREMENT_KINDS as readonly string[]).includes(
      requirement?.kind?.kind as string
    )
  );

/** A brand-new draft of the policy set that the studio points at. */
export const newDraft = (policyId: string): StudioPolicy => ({
  id: policyId,
  version: 'draft-1',
  schema_version: '1.0.0',
  requirements: [
    {
      kind: {
        kind: 'dimension',
        dimension: 'inventory_completeness',
        accepts: ['COMPLETE'],
      },
      on_failure: 'deny',
      overridable_by: null,
    },
  ],
});

/** The evidence snapshot the simulation starts with. */
export const defaultSnapshots = (): SimulatedSubject[] => [
  {
    subject: 'web-app',
    context: {
      version: '1.0.0',
      subject: {
        hrn: 'hrn:hodei:artifact::acme:oci:registry/web-app:latest',
        digest: null,
      },
      target: 'production',
      dimensions: [{ name: 'inventory_completeness', state: 'COMPLETE' }],
      capsule_hash: null,
      risk: {
        critical_known: 0,
        critical_reachable: 0,
        exposure: 'UNKNOWN',
        vex_accepted: 0,
        established: false,
        risk_version: 0,
      },
      exceptions: [],
      evidence_ids: ['sbom-0001'],
      assembled_at: '2026-11-20T00:00:00Z',
    },
  },
];
