const savePathEl = document.getElementById('save-path');
const changeFolderBtn = document.getElementById('change-folder-btn');
const openFolderBtn = document.getElementById('open-folder-btn');
const startBtn = document.getElementById('start-btn');
const stopBtn = document.getElementById('stop-btn');
const statusPanel = document.getElementById('status-panel');
const statusText = document.getElementById('status-text');
const qrContainer = document.getElementById('qr-container');
const qrList = document.getElementById('qr-list');
const uploadProgressContainer = document.getElementById('upload-progress-container');
const uploadFilename = document.getElementById('upload-filename');
const uploadProgressBar = document.getElementById('upload-progress-bar');

let currentSavePath = '';
let serverRunning = false;
let progressTimeout = null;

// Helper to access Tauri APIs safely
function getTauri() {
  if (window.__TAURI__) {
    return window.__TAURI__;
  }
  return null;
}

async function invokeTauri(cmd, args = {}) {
  const tauri = getTauri();
  if (tauri && tauri.core && tauri.core.invoke) {
    return await tauri.core.invoke(cmd, args);
  }
  console.warn(`Tauri core.invoke not found for ${cmd}`);
  return null;
}

// Setup event listener for upload progress
async function setupEventListeners() {
  const tauri = getTauri();
  if (tauri && tauri.event && tauri.event.listen) {
    await tauri.event.listen('upload-progress', (event) => {
      const data = event.payload;
      uploadProgressContainer.style.display = 'block';
      uploadFilename.textContent = `Receiving: ${data.filename} (${data.progress}%)`;
      uploadProgressBar.style.width = `${data.progress}%`;

      if (progressTimeout) clearTimeout(progressTimeout);
      if (data.progress >= 100) {
        progressTimeout = setTimeout(() => {
          uploadProgressContainer.style.display = 'none';
        }, 3000);
      }
    });
  }
}

async function init() {
  await setupEventListeners();

  const savedPath = localStorage.getItem('fileShare_savePath_tauri_v1');
  if (savedPath) {
    currentSavePath = savedPath;
  } else {
    try {
      currentSavePath = await invokeTauri('get_default_folder') || '';
      if (currentSavePath) {
        localStorage.setItem('fileShare_savePath_tauri_v1', currentSavePath);
      }
    } catch (err) {
      console.error('Failed to get default folder:', err);
    }
  }
  savePathEl.textContent = currentSavePath || 'No folder selected';
}

changeFolderBtn.addEventListener('click', async () => {
  try {
    const folder = await invokeTauri('select_folder');
    if (folder) {
      currentSavePath = folder;
      savePathEl.textContent = folder;
      localStorage.setItem('fileShare_savePath_tauri_v1', folder);
    }
  } catch (err) {
    alert('Failed to select folder: ' + err);
  }
});

openFolderBtn.addEventListener('click', async () => {
  if (currentSavePath) {
    try {
      await invokeTauri('open_folder', { path: currentSavePath });
    } catch (err) {
      alert('Failed to open folder: ' + err);
    }
  }
});

startBtn.addEventListener('click', async () => {
  if (!currentSavePath) {
    alert('Please select a save folder first.');
    return;
  }

  startBtn.textContent = 'Starting...';
  startBtn.disabled = true;

  try {
    const res = await invokeTauri('start_server', { savePath: currentSavePath });
    if (res && res.ok) {
      serverRunning = true;
      startBtn.style.display = 'none';
      stopBtn.style.display = 'block';
      statusPanel.classList.remove('offline');
      statusPanel.classList.add('online');
      statusText.textContent = 'Running';

      qrContainer.style.display = 'block';
      qrList.innerHTML = '';

      if (res.entries && res.entries.length > 0) {
        res.entries.forEach(entry => {
          const item = document.createElement('div');
          item.className = 'qr-item';
          const qrSrc = entry.qrDataUrl || entry.qr_data_url || '';
          item.innerHTML = `
            <img src="${qrSrc}" alt="QR">
            <p>${entry.name}</p>
            <a href="${entry.url}" class="qr-link">${entry.url}</a>
          `;
          const link = item.querySelector('.qr-link');
          link.addEventListener('click', (e) => {
            e.preventDefault();
            invokeTauri('open_url', { url: entry.url });
          });
          qrList.appendChild(item);
        });
      } else {
        qrList.innerHTML = '<p style="color:#8b949e">No active Wi-Fi / Ethernet connections found.</p>';
      }
    } else {
      alert('Failed to start server: ' + (res ? res.error : 'Unknown error'));
      startBtn.disabled = false;
    }
  } catch (err) {
    alert('Server start error: ' + err);
  } finally {
    startBtn.textContent = 'Start Sharing';
    startBtn.disabled = false;
  }
});

stopBtn.addEventListener('click', async () => {
  stopBtn.disabled = true;
  try {
    await invokeTauri('stop_server');
  } catch (err) {
    console.error('Stop server error:', err);
  }

  serverRunning = false;
  startBtn.style.display = 'block';
  stopBtn.style.display = 'none';
  statusPanel.classList.add('offline');
  statusPanel.classList.remove('online');
  statusText.textContent = 'Stopped';
  qrContainer.style.display = 'none';
  uploadProgressContainer.style.display = 'none';
  qrList.innerHTML = '';

  stopBtn.disabled = false;
});

// Run initialization
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}

