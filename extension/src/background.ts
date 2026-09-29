// Service worker: only makes the toolbar button open the side panel.
// It holds no document data and never talks to the network.
chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});
