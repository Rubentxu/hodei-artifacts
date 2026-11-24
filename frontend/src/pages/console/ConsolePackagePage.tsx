/**
 * The console of the catalog: the detail of an asset.
 *
 * The path of the asset arrives with the slashes inside it, because the
 * path is what names the content inside the repository.
 */

import { Link, useLocation, useParams } from 'react-router-dom';

import { PageHeader } from '@/components/layout/PageHeader';
import { PackageDetailView } from '@/features/console/components/PackageDetailView';
import { decodePackageRouteParams } from '@/features/console/lib/packageReference';

const ConsolePackagePage = () => {
  const { pathname } = useLocation();
  const { repository: paramRepo = '', '*': paramPath = '' } = useParams();
  const { repository, path } = decodePackageRouteParams(
    pathname,
    paramRepo,
    paramPath
  );

  return (
    <div className="p-6">
      <PageHeader
        title="Asset"
        subtitle={
          <Link to="/console/browse" className="text-blue-700 hover:underline">
            Back to the browser
          </Link>
        }
      />
      <PackageDetailView repository={repository} path={path} />
    </div>
  );
};

export default ConsolePackagePage;
