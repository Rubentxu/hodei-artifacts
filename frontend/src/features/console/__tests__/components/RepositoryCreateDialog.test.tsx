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
import { beforeEach, describe, expect, it } from 'vitest';

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
      http.get('/api/v1/capabilities', () =>
        HttpResponse.json(theCapabilities)
      )
    );
  });

  it('offers only the formats that the service serves', async () => {
    render(<RepositoryCreateDialog open onClose={() => {}} />);

    expect(await screen.findByRole('option', { name: 'Raw' })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: 'OCI' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'Cargo' })).not.toBeInTheDocument();
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
    await user.type(screen.getByLabelText('Upstream'), 'https://registry.example.org/');
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
});
