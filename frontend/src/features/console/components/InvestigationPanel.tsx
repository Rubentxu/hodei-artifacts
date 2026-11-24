/**
 * Contextual read-only investigation of an artefact.
 *
 * The panel queries `POST /api/v1/investigations` using the canonical HRN
 * provided by the service authority (`PackageView.capsule.subject`) and an
 * optional bounded question. It presents the report without mixing finding
 * classes, without hiding verification status or basis, and without claiming
 * exhaustiveness when coverage is partial. When the deployment has no active
 * investigator or model, it reports the limit honestly and never simulates
 * results.
 */

import { useState, type FormEvent } from 'react';
import { Search } from 'lucide-react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';

import { failureOf } from '../api/errors';
import type {
  InvestigationCoverage,
  InvestigationFindingKind,
  InvestigationVerificationStatus,
} from '../api/types';
import { useInvestigate } from '../hooks/useConsole';
import { FailureNotice } from './FailureNotice';

/** Maximum question length accepted by the investigation contract. */
const MAX_QUESTION_LENGTH = 2000;

/** Distinct human-readable labels for each finding class. */
const FINDING_KIND_LABELS: Record<InvestigationFindingKind, string> = {
  verified_fact: 'Verified fact',
  evidence_backed_claim: 'Evidence-backed claim',
  inference: 'Inference',
  uncertainty: 'Uncertainty',
  recommendation: 'Recommendation',
};

/** Distinct badge variants for each finding class. */
const FINDING_KIND_VARIANTS: Record<
  InvestigationFindingKind,
  'success' | 'info' | 'secondary' | 'warning' | 'default'
> = {
  verified_fact: 'success',
  evidence_backed_claim: 'info',
  inference: 'secondary',
  uncertainty: 'warning',
  recommendation: 'default',
};

/** Labels for each deterministic verification status. */
const STATUS_LABELS: Record<InvestigationVerificationStatus, string> = {
  verified: 'Verified',
  unverified: 'Unverified',
  contradicted: 'Contradicted',
  unverifiable: 'Unverifiable',
};

/** Badge variants for each deterministic verification status. */
const STATUS_VARIANTS: Record<
  InvestigationVerificationStatus,
  'success' | 'warning' | 'danger' | 'secondary'
> = {
  verified: 'success',
  unverified: 'warning',
  contradicted: 'danger',
  unverifiable: 'secondary',
};

const isPartialCoverage = (
  coverage: InvestigationCoverage
): coverage is { partial: { omitted: number } } =>
  typeof coverage === 'object' && coverage !== null && 'partial' in coverage;

export interface InvestigationPanelProps {
  /** Canonical HRN of the artefact from the backend authority. */
  subject: string;
}

export const InvestigationPanel = ({ subject }: InvestigationPanelProps) => {
  const [question, setQuestion] = useState('');
  const investigation = useInvestigate();

  const trimmedSubject = subject.trim();
  const trimmedQuestion = question.trim();
  const questionTooLong = question.length > MAX_QUESTION_LENGTH;
  const canInvestigate =
    trimmedSubject.length > 0 && !questionTooLong && !investigation.isPending;

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!canInvestigate) return;
    try {
      await investigation.mutateAsync(
        trimmedQuestion.length > 0
          ? { subject: trimmedSubject, question: trimmedQuestion }
          : { subject: trimmedSubject }
      );
    } catch {
      // Failure state is rendered from `investigation.error`.
    }
  };

  const failure = investigation.error ? failureOf(investigation.error) : null;
  const report = investigation.data;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Investigation</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <p className="text-sm text-gray-600">
          Read-only query over recorded evidence and relations for this
          artefact. Investigation does not promote content or alter trust
          decisions.
        </p>

        <div className="rounded-md bg-gray-50 px-3 py-2 text-xs text-gray-700">
          <span className="font-medium text-gray-500">Subject HRN: </span>
          <span className="break-all font-mono">
            {trimmedSubject || 'Unavailable'}
          </span>
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col gap-3">
          <Input
            label="Question (optional)"
            value={question}
            maxLength={MAX_QUESTION_LENGTH}
            onChange={event => setQuestion(event.target.value)}
            placeholder="Why is this artefact quarantined, or what evidence supports it?"
            helperText={`${question.length}/${MAX_QUESTION_LENGTH} characters. Optional; without a model, the question remains unanswered.`}
          />
          <div className="flex items-center gap-3">
            <Button
              type="submit"
              variant="outline"
              disabled={!canInvestigate}
              isLoading={investigation.isPending}
              leftIcon={<Search />}
            >
              Investigate
            </Button>
          </div>
        </form>

        {failure !== null && (
          <div className="flex flex-col gap-2">
            <FailureNotice failure={failure} />
            {(failure.kind === 'unavailable' || failure.kind === 'absence') && (
              <p className="text-sm text-gray-600">
                Investigation is not available on this deployment (the
                investigator or model is disabled, or the subject could not be
                reached). No results were simulated.
              </p>
            )}
          </div>
        )}

        {report && (
          <div className="flex flex-col gap-4 border-t border-gray-200 pt-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex flex-wrap items-center gap-2">
                {isPartialCoverage(report.coverage) ? (
                  <Badge variant="warning">
                    Partial coverage ({report.coverage.partial.omitted} omitted)
                  </Badge>
                ) : (
                  <Badge variant="success">Complete coverage</Badge>
                )}
                <span className="text-xs text-gray-500">
                  Schema v{report.schema_version} ·{' '}
                  {report.read_set.stamps.length} read stamp(s) ·{' '}
                  {report.read_set.generations.length} source generation(s)
                </span>
              </div>
            </div>

            {isPartialCoverage(report.coverage) && (
              <div
                role="status"
                className="rounded-md border border-yellow-200 bg-yellow-50 px-3 py-2 text-sm text-yellow-800"
              >
                Partial coverage: {report.coverage.partial.omitted} fact(s) were
                omitted by the deployment budget. This report is not exhaustive.
              </div>
            )}

            {report.findings.length === 0 ? (
              <p className="text-sm text-gray-600">
                No findings were returned for this artefact.
              </p>
            ) : (
              <ul className="flex flex-col gap-3">
                {report.findings.map((finding, index) => (
                  <li
                    key={`${finding.kind}-${index}`}
                    className="flex flex-col gap-2 rounded-md border border-gray-200 p-3"
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <Badge variant={FINDING_KIND_VARIANTS[finding.kind]}>
                        {FINDING_KIND_LABELS[finding.kind]}
                      </Badge>
                      <Badge variant={STATUS_VARIANTS[finding.status]}>
                        {STATUS_LABELS[finding.status]}
                      </Badge>
                    </div>
                    <p className="text-sm text-gray-900">{finding.statement}</p>
                    <p className="text-xs text-gray-600">
                      <span className="font-medium">Basis:</span>{' '}
                      {finding.basis}
                    </p>
                    <div className="text-xs text-gray-600">
                      <span className="font-medium">Citations: </span>
                      {finding.citations.length === 0 ? (
                        <span>none</span>
                      ) : (
                        <span className="font-mono">
                          {finding.citations.join(', ')}
                        </span>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
};
