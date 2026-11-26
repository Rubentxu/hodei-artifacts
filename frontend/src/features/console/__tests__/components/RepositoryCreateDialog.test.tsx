/**
 * The creation of a repository of the catalog.
 *
 * The form offers what the API says it can create. A console that keeps a
 * table of formats and modalities of its own would offer a repository
 * that the service does not serve, and the person would find out only
 * after filling the form in.
 */

import { http, HttpResponse } from 'msw';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';

import { RepositoryCreateDialog } from '../../components/RepositoryCreateDialog';

/** What the API serves. */
const theCapabilities = {
  formats: [
    { name: 'raw', modes: ['hosted', 'proxy', 'group'] },
    { name: 'oci', modes: ['hosted'] },
    { name: 'npm', modes: ['hosted', 'proxy', 'group'] },
  ],
};

describe('RepositoryCreateDialog', () => {
  beforeEach(() => {
    server.use(
      http.get('/api/v1/capabilities', () => HttpResponse.json(theCapabilities))
    );
  });

  it('offers only the formats that the service serves', async () => {
    render(<RepositoryCreateDialog open onClose={() => {}} />);

    expect(
      await screen.findByRole('option', { name: 'Raw' })
    ).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'OCI' })).toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: 'Cargo' })
    ).not.toBeInTheDocument();
  });

  it('offers the modalities of the format that was chosen', async () => {
    const user = userEvent.setup();
    render(<RepositoryCreateDialog open onClose={() => {}} />);

    await screen.findByRole('option', { name: 'OCI' });
    const select = screen.getByRole('combobox');
    await user.selectOptions(select, 'oci');

    expect(screen.getAllByRole('radio')).toHaveLength(1);
    expect(screen.getByRole('radio', { name: /Hosted/ })).toBeInTheDocument();

    await user.selectOptions(select, 'raw');

    expect(screen.getAllByRole('radio')).toHaveLength(3);
    expect(screen.getByRole('radio', { name: /Proxy/ })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: /Group/ })).toBeInTheDocument();
  });

  it('asks for the upstream when the modality is a proxy', async () => {
    const user = userEvent.setup();
    render(<RepositoryCreateDialog open onClose={() => {}} />);

    await screen.findByRole('option', { name: 'npm' });
    await user.selectOptions(screen.getByRole('combobox'), 'npm');
    await user.click(screen.getByRole('radio', { name: /Proxy/ }));

    expect(await screen.findByLabelText('Upstream')).toBeInTheDocument();
    expect(screen.queryByLabelText('Members')).not.toBeInTheDocument();
  });

  it('names the upstream the way the upstream reads it', async () => {
    const user = userEvent.setup();
    let created: unknown = null;
    server.use(
      http.post('/api/v1/catalog/repositories', async ({ request }) => {
        created = await request.json();
        return HttpResponse.json(
          {
            name: 'MyLibs',
            format: 'raw',
            mode: 'proxy',
            channel: 'open',
          },
          { status: 201 }
        );
      })
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    await screen.findByRole('option', { name: 'Raw' });
    await user.selectOptions(screen.getByRole('combobox'), 'raw');
    await user.click(screen.getByRole('radio', { name: /Proxy/ }));
    await user.type(await screen.findByLabelText('Name'), 'MyLibs');
    await user.type(
      screen.getByLabelText('Upstream'),
      'https://registry.example.org/'
    );
    await user.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() =>
      expect(created).toEqual({
        name: 'MyLibs',
        format: 'raw',
        mode: 'proxy',
        settings: {
          proxy: {
            upstream: {
              id: 'mylibs',
              url: 'https://registry.example.org/',
              scopes: [],
            },
          },
        },
      })
    );
  });

  it('creates what the API accepts', async () => {
    const user = userEvent.setup();
    let created: unknown = null;
    server.use(
      http.post('/api/v1/catalog/repositories', async ({ request }) => {
        created = await request.json();
        return HttpResponse.json(
          {
            name: 'releases',
            format: 'npm',
            mode: 'hosted',
            channel: 'open',
          },
          { status: 201 }
        );
      })
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    await user.type(await screen.findByLabelText('Name'), 'releases');
    await user.selectOptions(screen.getByRole('combobox'), 'npm');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() =>
      expect(created).toEqual({
        name: 'releases',
        format: 'npm',
        mode: 'hosted',
        settings: {},
      })
    );
  });

  it('says what the API refused when the creation does not go through', async () => {
    const user = userEvent.setup();
    server.use(
      http.post('/api/v1/catalog/repositories', () =>
        HttpResponse.json(
          { error: 'invalid', message: 'the modality is not served' },
          { status: 400 }
        )
      )
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    await user.type(await screen.findByLabelText('Name'), 'images');
    await user.selectOptions(screen.getByRole('combobox'), 'oci');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The request was not understood'
    );
  });

  it('offers a newly supported format from capabilities without a frontend allowlist', async () => {
    const user = userEvent.setup();
    let created: unknown = null;
    server.use(
      http.get('/api/v1/capabilities', () =>
        HttpResponse.json({
          formats: [{ name: 'terraform', modes: ['hosted'] }],
        })
      ),
      http.post('/api/v1/catalog/repositories', async ({ request }) => {
        created = await request.json();
        return HttpResponse.json(
          {
            name: 'infra-modules',
            format: 'terraform',
            mode: 'hosted',
            channel: 'open',
          },
          { status: 201 }
        );
      })
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    expect(
      await screen.findByRole('option', { name: 'terraform' })
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText('Name'), 'infra-modules');
    await user.selectOptions(screen.getByRole('combobox'), 'terraform');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() =>
      expect(created).toEqual({
        name: 'infra-modules',
        format: 'terraform',
        mode: 'hosted',
        settings: {},
      })
    );
  });

  it('fails closed without fallback options when capabilities fail to load', async () => {
    server.use(
      http.get('/api/v1/capabilities', () =>
        HttpResponse.json(
          {
            error: 'unavailable',
            message: 'the capabilities are not available',
          },
          { status: 503 }
        )
      )
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The service cannot answer right now'
    );
    expect(
      screen.getByText(/Repository creation is unavailable/)
    ).toBeInTheDocument();
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
    expect(screen.queryByRole('radio')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
  });

  it('fails closed when capabilities report no formats', async () => {
    server.use(
      http.get('/api/v1/capabilities', () => HttpResponse.json({ formats: [] }))
    );

    render(<RepositoryCreateDialog open onClose={() => {}} />);

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'No repository formats are available in this deployment.'
    );
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
  });

  it('shows permission denial on 403 and does not close', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    server.use(
      http.post('/api/v1/catalog/repositories', () =>
        HttpResponse.json(
          { error: 'permission', message: 'denied by policy' },
          { status: 403 }
        )
      )
    );

    render(<RepositoryCreateDialog open onClose={onClose} />);

    await user.type(await screen.findByLabelText('Name'), 'restricted');
    await user.selectOptions(screen.getByRole('combobox'), 'oci');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You are not allowed to do this'
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it('shows conflict on 409 and does not close', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    server.use(
      http.post('/api/v1/catalog/repositories', () =>
        HttpResponse.json(
          { error: 'conflict', message: 'repository already exists' },
          { status: 409 }
        )
      )
    );

    render(<RepositoryCreateDialog open onClose={onClose} />);

    await user.type(await screen.findByLabelText('Name'), 'existing-repo');
    await user.selectOptions(screen.getByRole('combobox'), 'npm');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'It already exists'
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
