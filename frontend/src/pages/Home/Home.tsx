import { Card } from '../../components/ui/Card';

export const Home = () => {
  return (
    <div className="min-h-screen bg-gray-50 py-12">
      <div className="container mx-auto px-4">
        <div className="max-w-4xl mx-auto">
          <Card className="text-center">
            <h1 className="text-4xl font-bold text-gray-900 mb-4">
              Hodei Artifacts
            </h1>
            <p className="text-lg text-gray-600 mb-8">
              Next-generation artifact repository with enterprise-grade security
              and performance
            </p>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
              <Card variant="secondary" className="p-6">
                <h3 className="text-xl font-semibold mb-2">Secure</h3>
                <p className="text-gray-600">
                  Built with supply chain security and access control
                </p>
              </Card>
              <Card variant="secondary" className="p-6">
                <h3 className="text-xl font-semibold mb-2">Fast</h3>
                <p className="text-gray-600">
                  High-performance Rust backend with optimized storage
                </p>
              </Card>
              <Card variant="secondary" className="p-6">
                <h3 className="text-xl font-semibold mb-2">Scalable</h3>
                <p className="text-gray-600">
                  Horizontal scaling with distributed architecture
                </p>
              </Card>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
};
