import { Card } from '../../components/ui/Card'

export const Dashboard = () => {
  return (
    <div className="p-6">
      <h1 className="text-3xl font-bold text-gray-900 mb-8">Dashboard</h1>
      
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6 mb-8">
        <Card className="p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Total Artifacts</h3>
          <p className="text-3xl font-bold text-blue-600">1,234</p>
        </Card>
        
        <Card className="p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Repositories</h3>
          <p className="text-3xl font-bold text-green-600">12</p>
        </Card>
        
        <Card className="p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Users</h3>
          <p className="text-3xl font-bold text-purple-600">45</p>
        </Card>
        
        <Card className="p-6">
          <h3 className="text-lg font-semibold text-gray-900 mb-2">Storage Used</h3>
          <p className="text-3xl font-bold text-orange-600">2.5 GB</p>
        </Card>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="p-6">
          <h3 className="text-xl font-semibold text-gray-900 mb-4">Recent Activity</h3>
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-gray-600">New artifact uploaded</span>
              <span className="text-sm text-gray-500">2 minutes ago</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-gray-600">User login</span>
              <span className="text-sm text-gray-500">5 minutes ago</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-gray-600">Repository created</span>
              <span className="text-sm text-gray-500">1 hour ago</span>
            </div>
          </div>
        </Card>

        <Card className="p-6">
          <h3 className="text-xl font-semibold text-gray-900 mb-4">Quick Actions</h3>
          <div className="space-y-3">
            <button className="w-full text-left p-3 rounded-lg bg-blue-50 hover:bg-blue-100 text-blue-700">
              Upload Artifact
            </button>
            <button className="w-full text-left p-3 rounded-lg bg-green-50 hover:bg-green-100 text-green-700">
              Create Repository
            </button>
            <button className="w-full text-left p-3 rounded-lg bg-purple-50 hover:bg-purple-100 text-purple-700">
              Manage Users
            </button>
          </div>
        </Card>
      </div>
    </div>
  )
}