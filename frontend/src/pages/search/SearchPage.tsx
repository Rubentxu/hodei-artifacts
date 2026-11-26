import { PageHeader } from '@/components/layout/PageHeader';
import { BrowseView } from '@/features/console/components/BrowseView';

/**
 * Global catalog search page backed by the canonical BrowseView and consoleApi.
 */
const SearchPage = () => (
  <div className="p-6">
    <PageHeader
      title="Search"
      subtitle="Search for assets across the repositories you may read."
    />
    <BrowseView />
  </div>
);

export default SearchPage;
