/**
 * The console of the catalog: usage and quota limits of the authenticated account.
 */

import { PageHeader } from '@/components/layout/PageHeader';
import { AccountUsageView } from '@/features/console/components/AccountUsageView';

const ConsoleAccountUsagePage = () => (
  <div className="p-6">
    <PageHeader
      title="Account Usage"
      subtitle="Storage consumption, in-flight uploads, and quota limits for your account."
    />
    <AccountUsageView />
  </div>
);

export default ConsoleAccountUsagePage;
