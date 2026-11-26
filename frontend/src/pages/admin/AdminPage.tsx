import { Link } from 'react-router-dom';
import { Database, FlaskConical, KeyRound, Shield, Users } from 'lucide-react';
import { PageHeader } from '@/components/layout/PageHeader';

interface AdminDestination {
  title: string;
  description: string;
  to: string;
  badge: string;
  icon: typeof Database;
  note?: string;
}

const ADMIN_DESTINATIONS: readonly AdminDestination[] = [
  {
    title: 'Repository Registry',
    description:
      'Manage physical repository definitions and registry configuration via /api/v1/repositories.',
    to: '/repositories',
    badge: 'Registry Admin',
    icon: Database,
    note: 'Distinct from Catalog Repositories (/console/repositories).',
  },
  {
    title: 'User Management',
    description: 'Manage system users, roles, and directory attributes.',
    to: '/admin/users',
    badge: 'Identity',
    icon: Users,
  },
  {
    title: 'Access Policies',
    description: 'Review and manage authorization policies for repositories.',
    to: '/settings/policies',
    badge: 'Policies',
    icon: Shield,
  },
  {
    title: 'Policy Studio',
    description:
      'Edit, validate, simulate and activate the trust policy that decides promotions.',
    to: '/console/policy-studio',
    badge: 'Trust',
    icon: FlaskConical,
  },
  {
    title: 'API Tokens',
    description: 'Manage personal and programmatic API access tokens.',
    to: '/settings/tokens',
    badge: 'Credentials',
    icon: KeyRound,
  },
] as const;

const AdminPage = () => {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Administration"
        subtitle="Platform configuration, registry administration, identity, and access controls."
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {ADMIN_DESTINATIONS.map(item => {
          const Icon = item.icon;
          return (
            <Link
              key={item.to}
              to={item.to}
              className="group flex flex-col justify-between rounded-lg border border-[var(--hodei-border)] bg-[var(--hodei-surface)] p-5 shadow-xs transition-colors hover:border-[var(--hodei-teal)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--hodei-focus-ring)] focus-visible:ring-offset-2"
            >
              <div>
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <span className="inline-flex h-10 w-10 items-center justify-center rounded-md bg-[var(--hodei-surface-muted)] text-[var(--hodei-teal)]">
                      <Icon className="h-5 w-5" aria-hidden="true" />
                    </span>
                    <h2 className="text-lg font-semibold text-[var(--hodei-text)] group-hover:text-[var(--hodei-teal)]">
                      {item.title}
                    </h2>
                  </div>
                  <span className="rounded-full bg-[var(--hodei-surface-muted)] px-2.5 py-0.5 text-xs font-medium text-[var(--hodei-text-muted)]">
                    {item.badge}
                  </span>
                </div>
                <p className="mt-3 text-sm text-[var(--hodei-text-muted)]">
                  {item.description}
                </p>
              </div>
              {item.note && (
                <p className="mt-3 text-xs font-medium text-[var(--hodei-teal)]">
                  {item.note}
                </p>
              )}
            </Link>
          );
        })}
      </div>
    </div>
  );
};

export default AdminPage;
