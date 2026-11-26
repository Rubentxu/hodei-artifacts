import { http, HttpResponse } from 'msw';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { server } from '@/shared/test/mocks/server';
import { render } from '@/shared/test/test-utils';

import { RepositoriesView } from '../../components/RepositoriesView';

const renderRepositoriesView = () =>
  render(
    <MemoryRouter>
      <RepositoriesView />
    </MemoryRouter>
  );

describe('RepositoriesView', () => {
  it('refreshes the repository list after creating a repository from capabilities', async () => {
    const user = userEvent.setup();
    const repositories = [
      {
        name: 'releases',
        format: 'maven2',
        mode: 'hosted',
        channel: 'open',
      },
    ];

    server.use(
      http.get('/api/v1/catalog/repositories', () =>
        HttpResponse.json({ repositories })
      ),
      http.get('/api/v1/capabilities', () =>
        HttpResponse.json({
          formats: [{ name: 'terraform', modes: ['hosted'] }],
        })
      ),
      http.post('/api/v1/catalog/repositories', async ({ request }) => {
        const body = (await request.json()) as {
          name: string;
          format: string;
          mode: string;
        };
        const created = {
          name: body.name,
          format: body.format,
          mode: body.mode,
          channel: 'open',
        };
        repositories.push(created);
        return HttpResponse.json(created, { status: 201 });
      })
    );

    renderRepositoriesView();

    expect(await screen.findByText('releases')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /New repository/ }));
    expect(
      await screen.findByRole('option', { name: 'terraform' })
    ).toBeInTheDocument();

    await user.type(screen.getByLabelText('Name'), 'infra');
    await user.selectOptions(screen.getByRole('combobox'), 'terraform');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    );
    expect(await screen.findByText('infra')).toBeInTheDocument();
  });

  it('keeps the dialog open with an error and does not refresh on 403 or 409', async () => {
    const user = userEvent.setup();
    let listCalls = 0;

    server.use(
      http.get('/api/v1/catalog/repositories', () => {
        listCalls += 1;
        return HttpResponse.json({ repositories: [] });
      }),
      http.get('/api/v1/capabilities', () =>
        HttpResponse.json({
          formats: [{ name: 'oci', modes: ['hosted'] }],
        })
      ),
      http.post('/api/v1/catalog/repositories', () =>
        HttpResponse.json(
          { error: 'permission', message: 'forbidden' },
          { status: 403 }
        )
      )
    );

    renderRepositoriesView();

    await screen.findByText('There is no repository you may read');
    expect(listCalls).toBe(1);

    await user.click(screen.getByRole('button', { name: /New repository/ }));
    await user.type(await screen.findByLabelText('Name'), 'denied-repo');
    await user.selectOptions(screen.getByRole('combobox'), 'oci');
    await user.click(screen.getByRole('radio', { name: /Hosted/ }));
    await user.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'You are not allowed to do this'
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(listCalls).toBe(1);
  });
});
