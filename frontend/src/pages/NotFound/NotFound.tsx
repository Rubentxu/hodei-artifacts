import { Card } from '../../components/ui/Card';
import { Button } from '../../components/ui/Button';

export const NotFound = () => {
  return (
    <div className="min-h-screen bg-gray-50 flex items-center justify-center py-12 px-4 sm:px-6 lg:px-8">
      <div className="max-w-md w-full">
        <Card className="text-center p-8">
          <div className="mb-6">
            <h1 className="text-9xl font-bold text-gray-300">404</h1>
          </div>
          <h2 className="text-2xl font-bold text-gray-900 mb-4">
            Page not found
          </h2>
          <p className="text-gray-600 mb-6">
            Sorry, we couldn't find the page you're looking for.
          </p>
          <Button onClick={() => window.history.back()}>Go back</Button>
        </Card>
      </div>
    </div>
  );
};
