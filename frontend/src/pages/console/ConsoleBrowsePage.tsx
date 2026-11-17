/**
 * The console of the catalog: the browser of the assets.
 */

import { PageHeader } from '@/components/layout/PageHeader';
import { BrowseView } from '@/features/console/components/BrowseView';

const ConsoleBrowsePage = () => (
  <div className="p-6">
    <PageHeader
      title="Browse"
      subtitle="What your account holds, and what has been decided about each thing."
    />
    <BrowseView />
  </div>
);

export default ConsoleBrowsePage;
