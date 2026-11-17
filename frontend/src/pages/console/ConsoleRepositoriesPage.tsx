/**
 * The console of the catalog: the repositories of the account.
 */

import { PageHeader } from '@/components/layout/PageHeader';
import { RepositoriesView } from '@/features/console/components/RepositoriesView';

const ConsoleRepositoriesPage = () => (
  <div className="p-6">
    <PageHeader
      title="Repositories"
      subtitle="The repositories of your account that you may read."
    />
    <RepositoriesView />
  </div>
);

export default ConsoleRepositoriesPage;
