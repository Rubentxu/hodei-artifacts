import { useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { Card } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { useAuth } from '@/shared/stores/auth.store';
import { authApi } from '@/shared/api/auth';
import { useNotifications } from '@/shared/stores/ui.store';

const Login = () => {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(false);
  const [isLoading, setIsLoading] = useState(false);

  const { login } = useAuth();
  const { showError, showSuccess } = useNotifications();
  const navigate = useNavigate();
  const location = useLocation();

  const from = location.state?.from?.pathname || '/';

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);

    try {
      const response = await authApi.login({ email, password, rememberMe });

      login(response.user, response.token);
      showSuccess('Login successful', 'Welcome back!');

      // Redirect to the intended page or home
      navigate(from, { replace: true });
    } catch (error: any) {
      showError(
        'Login failed',
        error.message || 'Please check your credentials and try again.'
      );
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50 flex items-center justify-center py-12 px-4 sm:px-6 lg:px-8">
      <div className="max-w-md w-full space-y-8">
        <div className="text-center">
          <h2 className="mt-6 text-3xl font-extrabold text-gray-900">
            Sign in to Hodei Artifacts
          </h2>
          <p className="mt-2 text-sm text-gray-600">
            Enter your credentials to access your repositories
          </p>
        </div>

        <Card className="px-8 py-6">
          <form className="space-y-6" onSubmit={handleSubmit}>
            <Input
              label="Email address"
              type="email"
              value={email}
              onChange={e => setEmail(e.target.value)}
              placeholder="Enter your email"
              required
              disabled={isLoading}
              autoComplete="email"
            />

            <Input
              label="Password"
              type="password"
              value={password}
              onChange={e => setPassword(e.target.value)}
              placeholder="Enter your password"
              required
              disabled={isLoading}
              autoComplete="current-password"
            />

            <div className="flex items-center justify-between">
              <div className="flex items-center">
                <input
                  id="remember-me"
                  name="remember-me"
                  type="checkbox"
                  checked={rememberMe}
                  onChange={e => setRememberMe(e.target.checked)}
                  className="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
                  disabled={isLoading}
                />
                <label
                  htmlFor="remember-me"
                  className="ml-2 block text-sm text-gray-900"
                >
                  Remember me
                </label>
              </div>

              <div className="text-sm">
                <a
                  href="#"
                  className="font-medium text-blue-600 hover:text-blue-500"
                  onClick={e => {
                    e.preventDefault();
                    showSuccess(
                      'Feature coming soon',
                      'Password recovery will be available soon.'
                    );
                  }}
                >
                  Forgot your password?
                </a>
              </div>
            </div>

            <Button
              type="submit"
              className="w-full relative"
              disabled={isLoading}
            >
              {isLoading ? (
                <>
                  <Spinner size="sm" variant="white" className="mr-2" />
                  Signing in...
                </>
              ) : (
                'Sign in'
              )}
            </Button>
          </form>

          <div className="mt-6">
            <div className="relative">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-gray-300" />
              </div>
              <div className="relative flex justify-center text-sm">
                <span className="px-2 bg-white text-gray-500">Demo access</span>
              </div>
            </div>

            <div className="mt-4 grid grid-cols-2 gap-3">
              <Button
                variant="secondary"
                onClick={() => {
                  setEmail('admin@example.com');
                  setPassword('admin123');
                  showSuccess('Demo credentials', 'Using admin demo account');
                }}
                disabled={isLoading}
              >
                Admin Demo
              </Button>

              <Button
                variant="secondary"
                onClick={() => {
                  setEmail('user@example.com');
                  setPassword('user123');
                  showSuccess('Demo credentials', 'Using user demo account');
                }}
                disabled={isLoading}
              >
                User Demo
              </Button>
            </div>
          </div>
        </Card>

        <div className="text-center">
          <p className="text-sm text-gray-600">
            Don't have an account?{' '}
            <a
              href="#"
              className="font-medium text-blue-600 hover:text-blue-500"
              onClick={e => {
                e.preventDefault();
                showSuccess(
                  'Feature coming soon',
                  'User registration will be available soon.'
                );
              }}
            >
              Contact administrator
            </a>
          </p>
        </div>
      </div>
    </div>
  );
};

export default Login;
