/**
 * The Policy Studio: edit, validate, simulate and activate a trust policy.
 */

import { PageHeader } from '@/components/layout/PageHeader';
import { PolicyStudioView } from '@/features/studio/components/PolicyStudioView';
import {
  selectPrincipalSessionKey,
  useAuthStore,
} from '@/shared/stores/auth.store';

const ConsolePolicyStudioPage = () => {
  // A new principal gets a new studio: nothing of the previous session's
  // draft, head or history survives the switch.
  const session = useAuthStore(selectPrincipalSessionKey);
  return (
    <div className="p-6">
      <PageHeader
        title="Policy Studio"
        subtitle="The effects of a trust policy, measured by the server. A simulation changes nothing."
      />
      <PolicyStudioView key={session} />
    </div>
  );
};

export default ConsolePolicyStudioPage;
