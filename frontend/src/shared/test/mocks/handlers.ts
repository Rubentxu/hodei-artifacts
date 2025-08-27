import { http, HttpResponse } from 'msw';

// Mock data for testing
export const mockArtifacts = [
  {
    id: '1',
    name: 'test-artifact-1.0.0.jar',
    groupId: 'com.example',
    artifactId: 'test-artifact',
    version: '1.0.0',
    size: 1024,
    uploadDate: '2024-01-01T00:00:00Z',
    checksum: 'abc123def456',
  },
  {
    id: '2',
    name: 'demo-library-2.1.0.tgz',
    groupId: 'org.demo',
    artifactId: 'demo-library',
    version: '2.1.0',
    size: 2048,
    uploadDate: '2024-01-02T00:00:00Z',
    checksum: 'def456ghi789',
  },
];

export const mockUsers = [
  {
    id: '1',
    username: 'testuser',
    email: 'test@example.com',
    role: 'user',
    createdAt: '2024-01-01T00:00:00Z',
  },
  {
    id: '2',
    username: 'adminuser',
    email: 'admin@example.com',
    role: 'admin',
    createdAt: '2024-01-02T00:00:00Z',
  },
];

// API handlers
export const handlers = [
  // Artifact endpoints
  http.get('/api/artifacts', () => {
    return HttpResponse.json({
      artifacts: mockArtifacts,
      total: mockArtifacts.length,
      page: 1,
      limit: 10,
    });
  }),

  http.get('/api/artifacts/:id', ({ params }) => {
    const artifact = mockArtifacts.find(a => a.id === params.id);
    if (!artifact) {
      return HttpResponse.json(
        { error: 'Artifact not found' },
        { status: 404 }
      );
    }
    return HttpResponse.json(artifact);
  }),

  http.post('/api/artifacts', async ({ request }) => {
    const body = (await request.json()) as any;
    const newArtifact = {
      id: (mockArtifacts.length + 1).toString(),
      ...body,
      uploadDate: new Date().toISOString(),
      checksum: 'mock-checksum-' + Math.random().toString(36).substring(2, 15),
    };
    mockArtifacts.push(newArtifact);
    return HttpResponse.json(newArtifact, { status: 201 });
  }),

  http.delete('/api/artifacts/:id', ({ params }) => {
    const index = mockArtifacts.findIndex(a => a.id === params.id);
    if (index === -1) {
      return HttpResponse.json(
        { error: 'Artifact not found' },
        { status: 404 }
      );
    }
    mockArtifacts.splice(index, 1);
    return HttpResponse.json({ success: true });
  }),

  // Authentication endpoints
  http.post('/api/auth/login', async ({ request }) => {
    const { username, password } = (await request.json()) as any;

    if (username === 'testuser' && password === 'password') {
      return HttpResponse.json({
        user: mockUsers[0],
        token: 'mock-jwt-token',
        expiresIn: 3600,
      });
    }

    return HttpResponse.json({ error: 'Invalid credentials' }, { status: 401 });
  }),

  http.post('/api/auth/register', async ({ request }) => {
    const { username, email, password } = (await request.json()) as any;

    const newUser = {
      id: (mockUsers.length + 1).toString(),
      username,
      email,
      role: 'user',
      createdAt: new Date().toISOString(),
    };

    mockUsers.push(newUser);

    return HttpResponse.json(
      {
        user: newUser,
        token: 'mock-jwt-token',
        expiresIn: 3600,
      },
      { status: 201 }
    );
  }),

  http.get('/api/auth/me', () => {
    return HttpResponse.json(mockUsers[0]);
  }),

  // User endpoints
  http.get('/api/users', () => {
    return HttpResponse.json({
      users: mockUsers,
      total: mockUsers.length,
    });
  }),

  http.get('/api/users/:id', ({ params }) => {
    const user = mockUsers.find(u => u.id === params.id);
    if (!user) {
      return HttpResponse.json({ error: 'User not found' }, { status: 404 });
    }
    return HttpResponse.json(user);
  }),

  // Search endpoints
  http.get('/api/search', ({ request }) => {
    const url = new URL(request.url);
    const query = url.searchParams.get('q');

    const results = mockArtifacts.filter(
      artifact =>
        artifact.name.toLowerCase().includes(query?.toLowerCase() || '') ||
        artifact.groupId.toLowerCase().includes(query?.toLowerCase() || '') ||
        artifact.artifactId.toLowerCase().includes(query?.toLowerCase() || '')
    );

    return HttpResponse.json({
      results,
      total: results.length,
      query,
    });
  }),

  // Health check
  http.get('/api/health', () => {
    return HttpResponse.json({
      status: 'ok',
      timestamp: new Date().toISOString(),
    });
  }),
];
