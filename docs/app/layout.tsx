import { RootProvider } from 'fumadocs-ui/provider';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import { source } from '@/lib/source';
import 'fumadocs-ui/style.css';
import './global.css';
import type { ReactNode } from 'react';

export const metadata = {
  title: {
    template: '%s | changesetter',
    default: 'changesetter',
  },
  description: 'Polyglot changeset management CLI',
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        <RootProvider>
          <DocsLayout
            tree={source.pageTree}
            nav={{ title: 'changesetter' }}
          >
            {children}
          </DocsLayout>
        </RootProvider>
      </body>
    </html>
  );
}
