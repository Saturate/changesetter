import Link from 'next/link';

export default function HomePage() {
  return (
    <main style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', minHeight: '100vh', gap: '1rem' }}>
      <h1 style={{ fontSize: '2rem', fontWeight: 'bold' }}>changesetter</h1>
      <p>Polyglot changeset management CLI</p>
      <Link href="/docs" style={{ color: '#0070f3', textDecoration: 'underline' }}>
        Read the docs
      </Link>
    </main>
  );
}
