import { RootProvider } from 'fumadocs-ui/provider';
import 'fumadocs-ui/style.css';
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
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}
