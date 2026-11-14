// Read store state using CI credentials; never upload a build or commit a Play edit.
import { sign } from 'node:crypto';

const [provider, build = '5011'] = process.argv.slice(2);
if (!/^\d+$/.test(build)) throw new Error('Build must be numeric');
const bundle = 'dev.aiworkspace.local';
const encode = value => Buffer.from(JSON.stringify(value)).toString('base64url');
function jwt(header, claims, key, ec = false) {
  const message = `${encode(header)}.${encode(claims)}`;
  return `${message}.${sign('sha256', Buffer.from(message), ec ? { key, dsaEncoding: 'ieee-p1363' } : key).toString('base64url')}`;
}
async function request(url, token, method = 'GET', body) {
  const response = await fetch(url, {
    method,
    headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), 'Content-Type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    // Do not log credentials, JWTs or full service responses.
    throw new Error(`${method} ${new URL(url).pathname}: HTTP ${response.status}`);
  }
  const text = await response.text();
  return text ? JSON.parse(text) : {};
}
const now = Math.floor(Date.now() / 1000);
if (provider === 'apple') {
  const token = jwt({ alg: 'ES256', kid: process.env.APPSTORE_API_KEY_ID, typ: 'JWT' },
    { iss: process.env.APPSTORE_API_ISSUER_ID, iat: now, exp: now + 600, aud: 'appstoreconnect-v1' },
    process.env.APPSTORE_API_KEY_P8, true);
  const base = 'https://api.appstoreconnect.apple.com/v1';
  const apps = await request(`${base}/apps?${new URLSearchParams({ 'filter[bundleId]': bundle })}`, token);
  const found = new Set();
  for (const app of apps.data) {
    const query = new URLSearchParams({ 'filter[app]': app.id, 'filter[version]': build, include: 'preReleaseVersion,buildBetaDetail', limit: '200' });
    const builds = await request(`${base}/builds?${query}`, token);
    for (const item of builds.data) {
      const related = name => builds.included?.find(x => x.id === item.relationships?.[name]?.data?.id && x.type === item.relationships?.[name]?.data?.type)?.attributes;
      const version = related('preReleaseVersion');
      const beta = related('buildBetaDetail');
      console.log(JSON.stringify({ platform: version?.platform, version: version?.version, build: item.attributes.version,
        processingState: item.attributes.processingState, expired: item.attributes.expired,
        internalBuildState: beta?.internalBuildState, externalBuildState: beta?.externalBuildState,
        usesNonExemptEncryption: item.attributes.usesNonExemptEncryption }));
      if (item.attributes.processingState === 'VALID' && !item.attributes.expired) found.add(version?.platform);
    }
  }
  if (!found.has('IOS') || !found.has('MAC_OS')) throw new Error('Both IOS and MAC_OS must have a VALID, unexpired build');
} else if (provider === 'google') {
  const account = JSON.parse(process.env.PLAY_SERVICE_ACCOUNT_JSON);
  const assertion = jwt({ alg: 'RS256', typ: 'JWT' }, { iss: account.client_email,
    scope: 'https://www.googleapis.com/auth/androidpublisher', aud: 'https://oauth2.googleapis.com/token', iat: now, exp: now + 600 }, account.private_key);
  const auth = await fetch('https://oauth2.googleapis.com/token', { method: 'POST',
    body: new URLSearchParams({ grant_type: 'urn:ietf:params:oauth:grant-type:jwt-bearer', assertion }), signal: AbortSignal.timeout(30_000) });
  if (!auth.ok) throw new Error(`Google authentication: HTTP ${auth.status}`);
  const { access_token: token } = await auth.json();
  const base = `https://androidpublisher.googleapis.com/androidpublisher/v3/applications/${bundle}/edits`;
  const edit = await request(base, token, 'POST', {});
  try {
    const track = await request(`${base}/${edit.id}/tracks/alpha`, token);
    console.log(JSON.stringify({ track: track.track, releases: track.releases?.map(r => ({ name: r.name, status: r.status, versionCodes: r.versionCodes })) }));
    if (!track.releases?.some(r => r.status === 'completed' && r.versionCodes?.includes(build))) throw new Error(`Build ${build} is not completed on alpha`);
  } finally {
    await request(`${base}/${edit.id}`, token, 'DELETE');
  }
} else {
  throw new Error('Provider must be apple or google');
}
