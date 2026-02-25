self.addEventListener('push', (event) => {
  let data = { title: 'KiteAgent', body: '' };
  try {
    data = event.data ? event.data.json() : data;
  } catch (_) {}
  event.waitUntil(
    self.registration.showNotification(data.title || 'KiteAgent', {
      body: data.body || 'Kite conditions update',
      icon: '/icon.png',
      badge: '/badge.png'
    })
  );
});
