#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    platform::windows::WindowExtWindows,
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

const DISCORD_URL: &str = "https://discord.com/login";

// Чистый User-Agent Google Chrome на Windows
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

// Стабильные флаги Chromium для Discord (аппаратные кодеки стрима + защита громкости Windows)
const CHROMIUM_ADDITIONAL_ARGS: &str = "\
    --enable-features=SharedArrayBuffer,WebRtcHWDecoding,WebRtcHWEncoding \
    --disable-features=WebRtcAllowInputVolumeChanges";

const VENCORD_BUNDLE: &str = include_str!("../vencord.bundle.js");

const VENCORD_POLYFILL: &str = r#"
var unsafeWindow = typeof window !== 'undefined' ? window : globalThis;
window.unsafeWindow = unsafeWindow;
globalThis.unsafeWindow = unsafeWindow;

(function() {
    const origDef = Object.defineProperty;
    Object.defineProperty = function(obj, prop, desc) {
        if (obj === unsafeWindow && prop === 'Vencord') {
            if (desc) desc.configurable = true;
            try {
                return origDef.call(Object, obj, prop, desc);
            } catch(e) {
                return obj;
            }
        }
        return origDef.call(Object, obj, prop, desc);
    };
})();

if (typeof GM_xmlhttpRequest === 'undefined') {
    window.GM_xmlhttpRequest = function(opts) {
        fetch(opts.url, {
            method: opts.method || (opts.data ? 'POST' : 'GET'),
            body: opts.data,
            headers: opts.headers
        }).then(async res => {
            const blob = await res.blob();
            const headerStr = Array.from(res.headers.entries()).map(([k,v]) => `${k}: ${v}`).join('\r\n');
            const resp = {
                response: blob,
                responseText: '',
                status: res.status,
                statusText: res.statusText,
                responseHeaders: headerStr
            };
            if (opts.onload) opts.onload(resp);
        }).catch(err => {
            if (opts.onerror) opts.onerror(err);
        });
    };
    var GM_xmlhttpRequest = window.GM_xmlhttpRequest;
}

try {
    let s = JSON.parse(localStorage.getItem('VencordSettings') || '{}');
    if (!s.plugins) s.plugins = {};
    delete s.plugins.CustomcordTitleBar;
    delete s.plugins.DesktopTitleBar;
    s.plugins.MessageLogger = Object.assign({ enabled: true }, s.plugins.MessageLogger);
    s.plugins.ShowHiddenChannels = Object.assign({ enabled: true }, s.plugins.ShowHiddenChannels);
    s.plugins.UserVoiceShow = Object.assign({ enabled: true }, s.plugins.UserVoiceShow);
    s.plugins.FakeNitro = Object.assign({ enabled: true }, s.plugins.FakeNitro);
    s.plugins.WebScreenShare = Object.assign({ enabled: true }, s.plugins.WebScreenShare);
    s.plugins.WebScreenShareFixes = Object.assign({ enabled: true }, s.plugins.WebScreenShareFixes);
    s.plugins.WebKeybinds = Object.assign({ enabled: true }, s.plugins.WebKeybinds);
    localStorage.setItem('VencordSettings', JSON.stringify(s));
} catch(e) {}
"#;

const CUSTOMCORD_INJECTION_SCRIPT: &str = r#"
(function() {
    if (window.top !== window) return;
    try {
        if (window.ipc) window.ipc.postMessage('log: INJECTION SCRIPT STARTED at ' + window.location.href);
    } catch(e) {}

    window.addEventListener('error', (e) => {
        try {
            if (window.ipc) window.ipc.postMessage('log: JS_ERR: ' + e.message + ' at ' + e.filename + ':' + e.lineno);
        } catch(_) {}
    });

    window.addEventListener('unhandledrejection', (e) => {
        try {
            if (window.ipc) window.ipc.postMessage('log: UNHANDLED_REJECTION: ' + (e.reason && (e.reason.stack || e.reason.message || e.reason)));
        } catch(_) {}
    });
    window.VesktopNative = window.VesktopNative || {
        win: {
            close: () => window.ipc && window.ipc.postMessage('close'),
            minimize: () => window.ipc && window.ipc.postMessage('minimize'),
            maximize: () => window.ipc && window.ipc.postMessage('maximize'),
            focus: () => {}
        }
    };

    // Управление масштабированием интерфейса (Zoom Level 50%-200%)
    const ZOOM_LEVELS = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];
    let currentZoom = 100;
    try {
        const savedZoom = localStorage.getItem('customcord_zoom');
        if (savedZoom) {
            const parsed = parseInt(savedZoom, 10);
            if (ZOOM_LEVELS.includes(parsed)) {
                currentZoom = parsed;
            }
        }
        if (window.ipc) window.ipc.postMessage('zoom:' + currentZoom);
    } catch(_) {}

    function getZoomPct(val) {
        return ((val - 50) / 150) * 100;
    }

    function setZoom(val) {
        currentZoom = val;
        try {
            localStorage.setItem('customcord_zoom', val.toString());
        } catch(_) {}
        if (window.ipc) {
            window.ipc.postMessage('zoom:' + val);
        }
        updateZoomUI();
    }

    function zoomStep(delta) {
        let idx = ZOOM_LEVELS.indexOf(currentZoom);
        if (idx === -1) {
            idx = 5;
            let minDiff = 9999;
            for (let i = 0; i < ZOOM_LEVELS.length; i++) {
                const diff = Math.abs(ZOOM_LEVELS[i] - currentZoom);
                if (diff < minDiff) {
                    minDiff = diff;
                    idx = i;
                }
            }
        }
        const newIdx = Math.max(0, Math.min(ZOOM_LEVELS.length - 1, idx + delta));
        setZoom(ZOOM_LEVELS[newIdx]);
    }

    function updateZoomUI() {
        const section = document.getElementById('customcord-zoom-section');
        if (!section) return;

        const pct = getZoomPct(currentZoom);
        const railFill = section.querySelector('.customcord-zoom-rail-fill');
        if (railFill) {
            railFill.style.width = 'calc(12px + (100% - 24px) * ' + (pct / 100) + ')';
        }

        const grabber = section.querySelector('.customcord-zoom-grabber');
        if (grabber) {
            grabber.style.left = 'calc(12px + (100% - 24px) * ' + (pct / 100) + ')';
            grabber.setAttribute('aria-valuenow', currentZoom.toString());
        }

        const markEls = section.querySelectorAll('.customcord-zoom-mark-val');
        markEls.forEach(el => {
            const v = parseInt(el.getAttribute('data-value'), 10);
            if (v === currentZoom) {
                el.classList.add('customcord-zoom-mark-active');
            } else {
                el.classList.remove('customcord-zoom-mark-active');
            }
        });

        const notchEls = section.querySelectorAll('.customcord-zoom-notch');
        notchEls.forEach(el => {
            const v = parseInt(el.getAttribute('data-value'), 10);
            if (v <= currentZoom) {
                el.classList.add('customcord-zoom-notch-filled');
            } else {
                el.classList.remove('customcord-zoom-notch-filled');
            }
        });
    }

    function findInsertionTarget() {
        const settings = document.querySelector('[class*="standardSidebarView"], [class*="contentRegion"], [class*="settingsPage"], [class*="sidebarRegion"]');
        if (!settings) return null;

        const content = settings.querySelector('[class*="contentColumn"], [class*="customColumn"], [class*="contentRegion"]') || settings;

        const headings = content.querySelectorAll('h1, h2, h3, h4, h5, [class*="title_"], [class*="heading_"], [class*="h3_"], [class*="h5_"], [role="heading"], label');
        let titleEl = null;
        for (const h of headings) {
            const t = (h.textContent || '').replace(/[\s\u00A0]+/g, ' ').trim();
            if (t.includes('Расстояние между группами') || t.includes('Message Group Spacing')) {
                titleEl = h;
                break;
            }
        }

        if (!titleEl) {
            const sliders = content.querySelectorAll('[role="slider"], [class*="slider_"]');
            for (const s of sliders) {
                const parentBlock = s.closest('[class*="item_"], [class*="section_"], div') || s.parentElement;
                if (parentBlock && parentBlock.textContent.includes('0px') && parentBlock.textContent.includes('24px') && !parentBlock.textContent.includes('Уровень масштабирования')) {
                    titleEl = parentBlock;
                    break;
                }
            }
        }

        if (!titleEl) return null;

        const isRu = (document.body.textContent || '').includes('Настройки') || (titleEl.textContent || '').includes('Расстояние');

        let cur = titleEl;
        while (cur && cur.parentElement && cur.parentElement !== content && cur.parentElement !== document.body) {
            if (cur.nextElementSibling) {
                const nextCls = (cur.nextElementSibling.className || '').toString();
                if (nextCls.includes('divider') || cur.nextElementSibling.id === 'customcord-zoom-section' || cur.nextElementSibling.tagName === 'HR') {
                    return { insertBefore: cur.nextElementSibling, titleEl, lang: isRu ? 'ru' : 'en' };
                }
            }
            const curText = (cur.textContent || '').replace(/[\s\u00A0]+/g, ' ');
            if (curText.includes('16px') && curText.includes('24px')) {
                if (cur.nextElementSibling) {
                    return { insertBefore: cur.nextElementSibling, titleEl, lang: isRu ? 'ru' : 'en' };
                }
                return { insertAfter: cur, titleEl, lang: isRu ? 'ru' : 'en' };
            }
            cur = cur.parentElement;
        }

        return { insertAfter: cur || titleEl.parentElement, titleEl, lang: isRu ? 'ru' : 'en' };
    }

    function injectZoomControl() {
        const settings = document.querySelector('[class*="standardSidebarView"], [class*="contentRegion"], [class*="settingsPage"], [class*="sidebarRegion"]');
        if (!settings) {
            const existing = document.getElementById('customcord-zoom-section');
            if (existing) existing.remove();
            return;
        }

        const found = findInsertionTarget();
        if (!found) return;

        let section = document.getElementById('customcord-zoom-section');
        if (section) {
            let inPlace = false;
            if (found.insertBefore && section.nextElementSibling === found.insertBefore) {
                inPlace = true;
            } else if (found.insertAfter && section.previousElementSibling === found.insertAfter) {
                inPlace = true;
            }

            if (!inPlace) {
                if (found.insertBefore && found.insertBefore.parentElement) {
                    found.insertBefore.parentElement.insertBefore(section, found.insertBefore);
                } else if (found.insertAfter) {
                    found.insertAfter.insertAdjacentElement('afterend', section);
                }
            }
            updateZoomUI();
            return;
        }

        const isRu = found.lang === 'ru';
        const titleText = isRu ? 'Уровень масштабирования' : 'Zoom Level';
        const descText = isRu ? 'Измените масштаб интерфейса. Также можно использовать ctrl +/-.' : 'Change the zoom level of the interface. You can also use ctrl +/-.';

        section = document.createElement('div');
        section.id = 'customcord-zoom-section';
        section.className = 'customcord-zoom-section';

        const titleCls = (found.titleEl && found.titleEl.className) ? found.titleEl.className : '';

        let marksHtml = '';
        let notchesHtml = '';
        for (let i = 0; i < ZOOM_LEVELS.length; i++) {
            const val = ZOOM_LEVELS[i];
            const pct = getZoomPct(val);
            const isDefault = (val === 100);
            const defaultCls = isDefault ? ' customcord-zoom-mark-default' : '';
            marksHtml += `<div class="customcord-zoom-mark-val${defaultCls}" data-value="${val}" style="left: calc(12px + (100% - 24px) * ${pct / 100});">${val}</div>`;
            notchesHtml += `<div class="customcord-zoom-notch" data-value="${val}" style="left: calc(12px + (100% - 24px) * ${pct / 100});"></div>`;
        }

        section.innerHTML = `
            <h3 class="customcord-zoom-title ${titleCls}">${titleText}</h3>
            <div class="customcord-zoom-desc">${descText}</div>
            <div class="customcord-zoom-slider-container">
                <div class="customcord-zoom-marks">
                    ${marksHtml}
                </div>
                <div class="customcord-zoom-track-wrap" role="slider" aria-label="${titleText}" aria-valuemin="50" aria-valuemax="200" aria-valuenow="${currentZoom}" tabindex="0">
                    <div class="customcord-zoom-rail-bg"></div>
                    <div class="customcord-zoom-rail-fill"></div>
                    <div class="customcord-zoom-notches">
                        ${notchesHtml}
                    </div>
                    <div class="customcord-zoom-grabber"></div>
                </div>
            </div>
        `;

        const trackWrap = section.querySelector('.customcord-zoom-track-wrap');

        function updateFromMouse(e) {
            const rect = trackWrap.getBoundingClientRect();
            const available = rect.width - 24;
            if (available <= 0) return;
            const relX = e.clientX - rect.left - 12;
            const clickPct = Math.max(0, Math.min(100, (relX / available) * 100));

            let bestVal = ZOOM_LEVELS[0];
            let minDiff = 999999;
            for (let i = 0; i < ZOOM_LEVELS.length; i++) {
                const v = ZOOM_LEVELS[i];
                const vPct = getZoomPct(v);
                const diff = Math.abs(vPct - clickPct);
                if (diff < minDiff) {
                    minDiff = diff;
                    bestVal = v;
                }
            }
            if (bestVal !== currentZoom) {
                setZoom(bestVal);
            }
        }

        let isDragging = false;
        trackWrap.addEventListener('mousedown', (e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            e.stopPropagation();
            isDragging = true;
            document.body.classList.add('customcord-zoom-grabbing');
            updateFromMouse(e);

            const onMouseMove = (ev) => {
                if (!isDragging) return;
                updateFromMouse(ev);
            };

            const onMouseUp = () => {
                isDragging = false;
                document.body.classList.remove('customcord-zoom-grabbing');
                document.removeEventListener('mousemove', onMouseMove);
                document.removeEventListener('mouseup', onMouseUp);
            };

            document.addEventListener('mousemove', onMouseMove);
            document.addEventListener('mouseup', onMouseUp);
        });

        trackWrap.addEventListener('keydown', (e) => {
            if (e.key === 'ArrowRight' || e.key === 'ArrowUp') {
                e.preventDefault();
                zoomStep(1);
            } else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') {
                e.preventDefault();
                zoomStep(-1);
            }
        });

        section.querySelectorAll('.customcord-zoom-mark-val').forEach(el => {
            el.addEventListener('click', (e) => {
                e.stopPropagation();
                const v = parseInt(el.getAttribute('data-value'), 10);
                if (!isNaN(v)) {
                    setZoom(v);
                }
            });
        });

        if (found.insertBefore && found.insertBefore.parentElement) {
            found.insertBefore.parentElement.insertBefore(section, found.insertBefore);
        } else if (found.insertAfter) {
            found.insertAfter.insertAdjacentElement('afterend', section);
        }
        updateZoomUI();

        if (window.ipc && !window.__customcord_logged_zoom_injected) {
            window.__customcord_logged_zoom_injected = true;
            window.ipc.postMessage('log: ZOOM_CONTROL_INJECTED_SUCCESSFULLY');
        }
    }

    // 1. Позиционирование кнопок окна 1:1 в точности по оригиналу (нативные координаты Windows / Discord Desktop 1920x1080):
    // [Почта] (центр 175.5px)  [Справка] (центр 140px)  [—] (центр 88px)  [□] (центр 52px)  [✕] (центр 15px)
    function injectHeaderControls() {
        // Создаём панель кнопок окна
        let controls = document.getElementById('customcord-header-controls');
        if (!controls) {
            controls = document.createElement('div');
            controls.id = 'customcord-header-controls';
            controls.innerHTML = `
                <div id="customcord-btn-min" class="customcord-header-win-btn" title="Свернуть">
                    <svg width="11" height="1" viewBox="0 0 11 1"><rect width="11" height="1" fill="currentColor"/></svg>
                </div>
                <div id="customcord-btn-max" class="customcord-header-win-btn" title="Развернуть">
                    <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1"/></svg>
                </div>
                <div id="customcord-btn-close" class="customcord-header-win-btn customcord-header-win-btn-close" title="Закрыть">
                    <svg width="10" height="10" viewBox="0 0 10 10"><path stroke="currentColor" stroke-width="1" stroke-linecap="square" d="M1 1 L9 9 M9 1 L1 9"/></svg>
                </div>
            `;

            controls.querySelector('#customcord-btn-min').addEventListener('click', (e) => {
                e.stopPropagation();
                if (window.ipc) window.ipc.postMessage('minimize');
            });
            controls.querySelector('#customcord-btn-max').addEventListener('click', (e) => {
                e.stopPropagation();
                if (window.ipc) window.ipc.postMessage('maximize');
            });
            controls.querySelector('#customcord-btn-close').addEventListener('click', (e) => {
                e.stopPropagation();
                if (window.ipc) window.ipc.postMessage('close');
            });

            const target = document.body || document.documentElement;
            if (target) target.appendChild(controls);
        }

        // Находим иконку Справки в верхней строке Discord
        const helpBtn = document.querySelector('[aria-label="Справка"], [aria-label="Help"], a[href*="support.discord.com"]');
        if (helpBtn && helpBtn.parentElement) {
            // Сдвиг иконок Справка/Почта ровно на 164px, чтобы центр Справки стоял на 140px от края (1:1 с оригиналом)
            helpBtn.parentElement.style.setProperty('padding-right', '164px', 'important');
        }
    }

    function ensureStyles() {
        if (document.getElementById('customcord-styles')) return;
        const target = document.head || document.documentElement;
        if (!target) return;

        const style = document.createElement('style');
        style.id = 'customcord-styles';
        style.textContent = `
            html, body {
                height: 100% !important;
                margin: 0 !important;
                padding: 0 !important;
                overflow: hidden !important;
                background-color: #313338 !important;
            }

            /* Панель управления окном строго в верхнем правом углу (высота 32px как в оригинальном заголовке Discord Desktop) */
            #customcord-header-controls {
                position: fixed !important;
                top: 0 !important;
                right: 0 !important;
                width: 106px !important;
                height: 32px !important;
                z-index: 99999999 !important;
                user-select: none !important;
                -webkit-user-select: none !important;
                pointer-events: none !important;
            }

            .customcord-header-win-btn {
                position: absolute !important;
                top: 0 !important;
                height: 32px !important;
                color: #949ba4 !important;
                cursor: pointer !important;
                transition: background-color 0.1s ease, color 0.1s ease !important;
                border-radius: 0 !important;
                margin: 0 !important;
                padding: 0 !important;
                pointer-events: auto !important;
            }

            /* Кнопка Закрыть: зона 0..34px, крестик 10x10 строго на d=10..20, y=10..20 (центр d=15.0, y=15.0) */
            #customcord-btn-close {
                right: 0 !important;
                width: 34px !important;
            }
            #customcord-btn-close svg {
                position: absolute !important;
                right: 10px !important;
                top: 10px !important;
                width: 10px !important;
                height: 10px !important;
            }

            /* Кнопка Развернуть: зона 34..70px, квадрат 10x10 строго на d=47..57, y=10..20 (центр d=52.0, y=15.0) */
            #customcord-btn-max {
                right: 34px !important;
                width: 36px !important;
            }
            #customcord-btn-max svg {
                position: absolute !important;
                right: 13px !important;
                top: 10px !important;
                width: 10px !important;
                height: 10px !important;
            }

            /* Кнопка Свернуть: зона 70..106px, полоска 11x1 строго на d=82.5..93.5, y=15 (центр d=88.0, y=15.0) */
            #customcord-btn-min {
                right: 70px !important;
                width: 36px !important;
            }
            #customcord-btn-min svg {
                position: absolute !important;
                right: 12.5px !important;
                top: 15px !important;
                width: 11px !important;
                height: 1px !important;
            }

            .customcord-header-win-btn:hover {
                background-color: #35373c !important;
                color: #dbdee1 !important;
            }

            .customcord-header-win-btn-close:hover {
                background-color: #da373c !important;
                color: #ffffff !important;
            }

            /* Отступ блока Справки и Почты на 164px (центр Справки ровно на 140px, Почты на 175.5px) */
            [class*="trailing_"],
            div:has(> a[href*="support.discord.com"]),
            div:has(> [aria-label="Справка"]),
            div:has(> [aria-label="Help"]) {
                padding-right: 164px !important;
            }

            /* Скрытие веб-баннеров и промо-уведомлений */
            [class*="downloadApps"], [class*="mobileAppNotice"], [class*="browserNotice"], a[href*="/download"] {
                display: none !important;
            }

            /* Фикс размера плашки скрытых каналов (ShowHiddenChannels) 1:1 как в оригинале */
            .vc-shc-logo,
            img[class*="vc-shc-logo"],
            [class*="vc-shc-"] img,
            [class*="vc-shc-container"] img {
                width: 130px !important;
                height: 130px !important;
                max-width: 130px !important;
                max-height: 130px !important;
                object-fit: contain !important;
                margin: 0 auto !important;
                display: block !important;
            }

            .vc-shc-container,
            [class*="vc-shc-container"] {
                display: flex !important;
                flex-direction: column !important;
                align-items: center !important;
                justify-content: center !important;
                text-align: center !important;
                min-height: 100% !important;
                padding: 20px !important;
                box-sizing: border-box !important;
            }

            /* Разрешаем полноценный drag-and-drop для серверов, папок и каналов Discord */
            [draggable="true"],
            [draggable="true"] * {
                -webkit-user-drag: element !important;
                user-drag: element !important;
            }
            nav[class*="guilds"],
            [data-list-id="guildsnav"],
            [class*="listItem"],
            [class*="blobContainer"] {
                -webkit-user-drag: auto !important;
            }

            ::-webkit-scrollbar {
                width: 8px !important;
                height: 8px !important;
            }
            ::-webkit-scrollbar-track {
                background: transparent !important;
            }
            ::-webkit-scrollbar-thumb {
                background: #1a1b1e !important;
                border-radius: 4px !important;
            }
            ::-webkit-scrollbar-thumb:hover {
                background: #111214 !important;
            }

            /* Zoom Level section */
            .customcord-zoom-section {
                display: block !important;
                position: relative !important;
                margin-top: 24px !important;
                margin-bottom: 24px !important;
                user-select: none !important;
                -webkit-user-select: none !important;
            }

            .customcord-zoom-title {
                font-size: 16px !important;
                font-weight: 600 !important;
                line-height: 20px !important;
                color: var(--header-primary, #f2f3f5) !important;
                margin: 0 0 8px 0 !important;
                letter-spacing: 0.02em !important;
                font-family: var(--font-primary, "gg sans", "Noto Sans", "Helvetica Neue", Helvetica, Arial, sans-serif) !important;
            }

            .customcord-zoom-desc {
                font-size: 14px !important;
                line-height: 18px !important;
                color: var(--text-muted, #949ba4) !important;
                margin: 0 0 20px 0 !important;
                font-family: var(--font-primary, "gg sans", "Noto Sans", "Helvetica Neue", Helvetica, Arial, sans-serif) !important;
            }

            .customcord-zoom-slider-container {
                position: relative !important;
                width: 100% !important;
            }

            .customcord-zoom-marks {
                position: relative !important;
                width: 100% !important;
                height: 20px !important;
                margin-bottom: 4px !important;
            }

            .customcord-zoom-mark-val {
                position: absolute !important;
                top: 0 !important;
                transform: translateX(-50%) !important;
                font-size: 12px !important;
                font-weight: 600 !important;
                line-height: 16px !important;
                color: var(--text-muted, #949ba4) !important;
                cursor: pointer !important;
                font-family: var(--font-primary, "gg sans", "Noto Sans", "Helvetica Neue", Helvetica, Arial, sans-serif) !important;
                transition: color 0.1s ease !important;
            }

            .customcord-zoom-mark-val:hover {
                color: var(--header-primary, #f2f3f5) !important;
            }

            .customcord-zoom-mark-default {
                color: #23a55a !important;
            }

            .customcord-zoom-mark-active {
                color: var(--header-primary, #f2f3f5) !important;
                font-weight: 700 !important;
            }

            .customcord-zoom-mark-default.customcord-zoom-mark-active {
                color: #23a55a !important;
                font-weight: 700 !important;
            }

            .customcord-zoom-track-wrap {
                position: relative !important;
                width: 100% !important;
                height: 40px !important;
                display: flex !important;
                align-items: center !important;
                cursor: pointer !important;
            }

            .customcord-zoom-rail-bg {
                position: absolute !important;
                left: 0 !important;
                right: 0 !important;
                top: 16px !important;
                height: 8px !important;
                border-radius: 4px !important;
                background-color: var(--background-modifier-accent, #4e5058) !important;
                pointer-events: none !important;
            }

            .customcord-zoom-rail-fill {
                position: absolute !important;
                left: 0 !important;
                top: 16px !important;
                height: 8px !important;
                border-radius: 4px !important;
                background-color: var(--brand-500, #5865F2) !important;
                pointer-events: none !important;
                transition: width 0.05s ease !important;
            }

            .customcord-zoom-notch {
                position: absolute !important;
                top: 12px !important;
                width: 2px !important;
                height: 16px !important;
                transform: translateX(-50%) !important;
                background-color: var(--background-modifier-accent, #4e5058) !important;
                pointer-events: none !important;
            }

            .customcord-zoom-notch-filled {
                background-color: rgba(255, 255, 255, 0.4) !important;
            }

            .customcord-zoom-grabber {
                position: absolute !important;
                top: 8px !important;
                width: 24px !important;
                height: 24px !important;
                border-radius: 50% !important;
                background-color: #ffffff !important;
                transform: translateX(-50%) !important;
                box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2) !important;
                cursor: grab !important;
                z-index: 3 !important;
                transition: left 0.05s ease, box-shadow 0.1s ease !important;
            }

            .customcord-zoom-grabber:hover,
            .customcord-zoom-grabber:active {
                box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4) !important;
            }

            .customcord-zoom-grabbing,
            .customcord-zoom-grabbing * {
                cursor: grabbing !important;
            }
        `;
        target.appendChild(style);
    }

    function initUI() {
        injectHeaderControls();
        ensureStyles();
        injectZoomControl();
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', initUI);
    } else {
        initUI();
    }
    setInterval(initUI, 300);

    // Перетаскивание окна за свободную верхнюю строку (высота 32px)
    document.addEventListener('mousedown', (e) => {
        if (e.button !== 0) return;
        const target = e.target;
        if (e.clientX <= 72 || target.closest('button, a, input, textarea, [role="button"], [class*="winButton"], [class*="iconWrapper"], [class*="clickable"], .customcord-header-win-btn, nav, [data-list-id="guildsnav"], [class*="guilds"], [draggable="true"], [class*="listItem"], [class*="blobContainer"]')) {
            return;
        }
        if (e.clientY <= 32) {
            if (window.ipc) window.ipc.postMessage('drag');
        }
    });

    document.addEventListener('dblclick', (e) => {
        if (e.button !== 0) return;
        const target = e.target;
        if (e.clientX <= 72 || target.closest('button, a, input, textarea, [role="button"], [class*="winButton"], [class*="iconWrapper"], [class*="clickable"], .customcord-header-win-btn, nav, [data-list-id="guildsnav"], [class*="guilds"], [draggable="true"], [class*="listItem"], [class*="blobContainer"]')) {
            return;
        }
        if (e.clientY <= 32) {
            if (window.ipc) window.ipc.postMessage('maximize');
        }
    });

    // 4. Форсирование 60 FPS для видеопотока экрана
    if (window.MediaStreamTrack) {
        if (MediaStreamTrack.prototype.getSettings) {
            const origGetSettings = MediaStreamTrack.prototype.getSettings;
            MediaStreamTrack.prototype.getSettings = function() {
                const s = origGetSettings.apply(this, arguments);
                if (this.kind === 'video' && s && (!s.frameRate || s.frameRate < 60)) {
                    s.frameRate = 60;
                }
                return s;
            };
        }

        if (MediaStreamTrack.prototype.applyConstraints) {
            const origApplyConstraints = MediaStreamTrack.prototype.applyConstraints;
            MediaStreamTrack.prototype.applyConstraints = async function(constraints) {
                if (this.kind === 'video' && constraints) {
                    if (typeof constraints.frameRate === 'object') {
                        constraints.frameRate.ideal = 60;
                        constraints.frameRate.max = 60;
                    } else if (constraints.frameRate) {
                        constraints.frameRate = 60;
                    }
                }
                return origApplyConstraints.call(this, constraints);
            };
        }
    }

    // 5. Снятие лимита битрейта WebRTC видеопотока экрана до 8.5 Мбит/с
    if (window.RTCRtpSender && RTCRtpSender.prototype.setParameters) {
        const origSetParameters = RTCRtpSender.prototype.setParameters;
        RTCRtpSender.prototype.setParameters = async function(params) {
            if (this.track && this.track.kind === 'video') {
                if (params && params.encodings) {
                    for (const enc of params.encodings) {
                        enc.maxBitrate = 8500000;
                        enc.maxFramerate = 60;
                    }
                }
            }
            return origSetParameters.apply(this, arguments);
        };
    }

    if (window.RTCPeerConnection && RTCPeerConnection.prototype.setLocalDescription) {
        const origSetLocalDescription = RTCPeerConnection.prototype.setLocalDescription;
        RTCPeerConnection.prototype.setLocalDescription = function(desc) {
            if (desc && desc.sdp) {
                desc.sdp = desc.sdp.replace(/a=framerate:[0-9.]+/g, 'a=framerate:60');
                desc.sdp = desc.sdp.replace(/(m=video [^\r\n]+[\r\n]+)/g, '$1b=AS:8500\r\nb=TIAS:8500000\r\n');
            }
            return origSetLocalDescription.apply(this, arguments);
        };
    }

    // 6. Чистый нативный микрофон WebRTC (1:1 как в Discord Desktop) и Push-to-Talk
    let cachedMediaEngineStore = null;
    let cachedMediaEngineActions = null;
    let cachedSoundUtils = null;
    let isPttActive = false;
    let currentPttMode = 'VOICE_ACTIVITY';
    let currentShortcutCode = 20; // По умолчанию CapsLock
    let shortcutInitialized = false;
    let pttReleaseTimeout = null;

    function getMediaEngineStore() {
        if (cachedMediaEngineStore) return cachedMediaEngineStore;
        if (window.MediaEngineStore) return (cachedMediaEngineStore = window.MediaEngineStore);
        if (window.Vencord && window.Vencord.Webpack) {
            try {
                const s = window.Vencord.Webpack.findStore("MediaEngineStore");
                if (s) return (cachedMediaEngineStore = s);
            } catch(e) {}
        }
        return cachedMediaEngineStore;
    }

    function getMediaEngineActions() {
        if (cachedMediaEngineActions) return cachedMediaEngineActions;
        if (window.Vencord && window.Vencord.Webpack) {
            try {
                const a = window.Vencord.Webpack.findByProps("setTemporarySelfMute", "toggleSelfMute");
                if (a) return (cachedMediaEngineActions = a);
            } catch(e) {}
        }
        return cachedMediaEngineActions;
    }

    function getSoundUtils() {
        if (cachedSoundUtils) return cachedSoundUtils;
        if (window.Vencord && window.Vencord.Webpack) {
            try {
                const s = window.Vencord.Webpack.findByProps("playSound", "createSound");
                if (s) return (cachedSoundUtils = s);
            } catch(e) {}
        }
        return cachedSoundUtils;
    }

    // Управление микрофоном через официальный метод Discord MediaEngine
    window.__customcord_ptt = function(active) {
        if (currentPttMode !== 'PUSH_TO_TALK') return;

        const actions = getMediaEngineActions();
        const soundUtils = getSoundUtils();

        if (active) {
            if (pttReleaseTimeout) {
                clearTimeout(pttReleaseTimeout);
                pttReleaseTimeout = null;
            }
            if (!isPttActive) {
                isPttActive = true;
                if (actions && typeof actions.setTemporarySelfMute === 'function') {
                    actions.setTemporarySelfMute(false);
                }
                if (soundUtils && typeof soundUtils.playSound === 'function') {
                    try { soundUtils.playSound('ptt_start'); } catch(e) {}
                }
            }
        } else {
            let releaseDelay = 20;
            try {
                const store = getMediaEngineStore();
                if (store && store.getModeOptions) {
                    const opts = store.getModeOptions();
                    if (opts && typeof opts.delay === 'number' && opts.delay >= 0) {
                        releaseDelay = opts.delay;
                    }
                }
            } catch(e) {}

            if (pttReleaseTimeout) clearTimeout(pttReleaseTimeout);
            pttReleaseTimeout = setTimeout(() => {
                isPttActive = false;
                if (currentPttMode === 'PUSH_TO_TALK') {
                    if (actions && typeof actions.setTemporarySelfMute === 'function') {
                        actions.setTemporarySelfMute(true);
                    }
                }
                if (soundUtils && typeof soundUtils.playSound === 'function') {
                    try { soundUtils.playSound('ptt_stop'); } catch(e) {}
                }
            }, releaseDelay);
        }
    };

    // Легковесная синхронизация режима (Voice Activity vs PTT) и бинда
    function checkVoiceSettings() {
        try {
            const store = getMediaEngineStore();
            if (!store) return;

            const mode = store.getMode ? store.getMode() : 'VOICE_ACTIVITY';
            if (mode !== currentPttMode) {
                currentPttMode = mode;
                const actions = getMediaEngineActions();
                if (actions && typeof actions.setTemporarySelfMute === 'function') {
                    if (currentPttMode === 'PUSH_TO_TALK') {
                        actions.setTemporarySelfMute(!isPttActive);
                    } else {
                        actions.setTemporarySelfMute(false);
                    }
                }
            }

            const opts = store.getModeOptions ? store.getModeOptions() : {};
            if (opts && opts.shortcut && opts.shortcut.length > 0) {
                const item = opts.shortcut[0];
                const devType = item[0] || 0;
                const code = item[1] || 20;
                if (!shortcutInitialized || code !== currentShortcutCode) {
                    shortcutInitialized = true;
                    currentShortcutCode = code;
                    if (window.ipc) {
                        window.ipc.postMessage('ptt_key:' + devType + ':' + code);
                    }
                }
            }
        } catch(e) {}
    }
    setInterval(checkVoiceSettings, 1000);

    // 8. Подавление браузерных шорткатов (F5, Ctrl+R, Ctrl+P, Ctrl+U) и поддержка Ctrl +/-/0 для зума
    window.addEventListener('keydown', (e) => {
        if (e.key === 'F5' || 
            (e.ctrlKey && (e.key === 'r' || e.key === 'R' || e.key === 'p' || e.key === 'P' || e.key === 'u' || e.key === 'U'))) {
            e.preventDefault();
            e.stopPropagation();
        }

        if (e.ctrlKey && !e.altKey) {
            if (e.key === '=' || e.key === '+' || e.code === 'Equal' || e.code === 'NumpadAdd') {
                e.preventDefault();
                e.stopPropagation();
                zoomStep(1);
            } else if (e.key === '-' || e.key === '_' || e.code === 'Minus' || e.code === 'NumpadSubtract') {
                e.preventDefault();
                e.stopPropagation();
                zoomStep(-1);
            } else if (e.key === '0' || e.code === 'Digit0' || e.code === 'Numpad0') {
                e.preventDefault();
                e.stopPropagation();
                setZoom(100);
            }
        }
    }, true);

    window.addEventListener('wheel', (e) => {
        if (e.ctrlKey) {
            e.preventDefault();
            e.stopPropagation();
            if (e.deltaY < 0) {
                zoomStep(1);
            } else if (e.deltaY > 0) {
                zoomStep(-1);
            }
        }
    }, { passive: false, capture: true });
})();
"#;

#[repr(C)]
struct PROCESSENTRY32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; 260],
}

// Принудительный сброс неиспользуемой физической памяти (Working Set)
// для нашего процесса и всего дерева дочерних процессов WebView2
fn trim_all_memory() {
    #[cfg(target_os = "windows")]
    unsafe {
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn GetCurrentProcessId() -> u32;
            fn SetProcessWorkingSetSize(
                hProcess: isize,
                dwMinimumWorkingSetSize: usize,
                dwMaximumWorkingSetSize: usize,
            ) -> i32;
            fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> isize;
            fn Process32FirstW(hSnapshot: isize, lppe: *mut PROCESSENTRY32W) -> i32;
            fn Process32NextW(hSnapshot: isize, lppe: *mut PROCESSENTRY32W) -> i32;
            fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> isize;
            fn CloseHandle(hObject: isize) -> i32;
        }

        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);

        let my_pid = GetCurrentProcessId();
        let snapshot = CreateToolhelp32Snapshot(0x00000002, 0); // TH32CS_SNAPPROCESS
        if snapshot != -1 && snapshot != 0 {
            let mut entry = PROCESSENTRY32W {
                dw_size: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                cnt_usage: 0,
                th32_process_id: 0,
                th32_default_heap_id: 0,
                th32_module_id: 0,
                cnt_threads: 0,
                th32_parent_process_id: 0,
                pc_pri_class_base: 0,
                dw_flags: 0,
                sz_exe_file: [0u16; 260],
            };

            let mut all_procs = Vec::new();
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    all_procs.push((entry.th32_process_id, entry.th32_parent_process_id));
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);

            let mut tree_pids = HashSet::new();
            tree_pids.insert(my_pid);

            let mut changed = true;
            while changed {
                changed = false;
                for (pid, ppid) in &all_procs {
                    if tree_pids.contains(ppid) && !tree_pids.contains(pid) {
                        tree_pids.insert(*pid);
                        changed = true;
                    }
                }
            }

            for pid in tree_pids {
                if pid != my_pid {
                    let h_proc = OpenProcess(0x0500, 0, pid);
                    if h_proc != 0 {
                        SetProcessWorkingSetSize(h_proc, usize::MAX, usize::MAX);
                        CloseHandle(h_proc);
                    }
                }
            }
        }
    }
}

// Фоновый поток:
// 1. Автоматически скрывает системную плашку "discord.com предоставляет доступ к вашему экрану"
// 2. Каждые 60 секунд принудительно сбрасывает закэшированную память процессов в Windows
fn start_background_optimizer(running: Arc<AtomicBool>) {
    #[cfg(target_os = "windows")]
    thread::spawn(move || {
        unsafe extern "system" {
            fn EnumWindows(
                lpEnumFunc: unsafe extern "system" fn(isize, isize) -> i32,
                lParam: isize,
            ) -> i32;
            fn GetClassNameW(hWnd: isize, lpClassName: *mut u16, nMaxCount: i32) -> i32;
            fn GetWindowTextW(hWnd: isize, lpString: *mut u16, nMaxCount: i32) -> i32;
            fn ShowWindow(hWnd: isize, nCmdShow: i32) -> i32;
            fn IsWindowVisible(hWnd: isize) -> i32;
        }

        unsafe extern "system" fn enum_window_callback(hwnd: isize, _lparam: isize) -> i32 {
            unsafe {
                if IsWindowVisible(hwnd) == 0 {
                    return 1;
                }

                let mut class_buf = [0u16; 256];
                let class_len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
                if class_len > 0 {
                    let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);
                    if class_name.starts_with("Chrome_WidgetWin_") {
                        let mut text_buf = [0u16; 512];
                        let text_len = GetWindowTextW(hwnd, text_buf.as_mut_ptr(), 512);
                        let title = if text_len > 0 {
                            String::from_utf16_lossy(&text_buf[..text_len as usize])
                        } else {
                            String::new()
                        };

                        if title.contains("предоставляет доступ к вашему экрану")
                            || title.contains("is sharing your screen")
                            || title.contains("is sharing a window")
                        {
                            ShowWindow(hwnd, 0); // SW_HIDE
                        }
                    }
                }
                1
            }
        }

        while running.load(Ordering::Relaxed) {
            unsafe {
                EnumWindows(enum_window_callback, 0);
            }
            thread::sleep(Duration::from_millis(1000));
        }
    });
}

fn get_vk_from_shortcut(device_type: i32, code: i32) -> i32 {
    if device_type == 0 {
        code
    } else if device_type == 1 {
        match code {
            0 => 0x01, // VK_LBUTTON
            1 => 0x02, // VK_RBUTTON
            2 => 0x04, // VK_MBUTTON
            3 => 0x05, // VK_XBUTTON1 (Mouse 4)
            4 => 0x06, // VK_XBUTTON2 (Mouse 5)
            _ => 0x05,
        }
    } else {
        20 // Default CapsLock
    }
}

// Глобальный перехватчик рации (Push-to-Talk)
// Работает в фоне во всех играх и окнах Windows
fn start_global_ptt_listener(
    running: Arc<AtomicBool>,
    target_vk: Arc<AtomicI32>,
    proxy: tao::event_loop::EventLoopProxy<AppCmd>,
) {
    #[cfg(target_os = "windows")]
    thread::spawn(move || {
        unsafe extern "system" {
            fn GetAsyncKeyState(vKey: i32) -> i16;
        }

        let mut was_pressed = false;

        while running.load(Ordering::Relaxed) {
            let vk = target_vk.load(Ordering::Relaxed);
            if vk > 0 {
                let state = unsafe { GetAsyncKeyState(vk) };
                let is_pressed = (state as u16 & 0x8000) != 0;

                if is_pressed && !was_pressed {
                    was_pressed = true;
                    let _ = proxy.send_event(AppCmd::PttDown);
                } else if !is_pressed && was_pressed {
                    was_pressed = false;
                    let _ = proxy.send_event(AppCmd::PttUp);
                }
            }

            thread::sleep(Duration::from_millis(8));
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum AppCmd {
    Minimize,
    Maximize,
    Close,
    Drag,
    PttDown,
    PttUp,
    Zoom(f64),
}

fn log_debug(msg: &str) {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("debug.log")))
        .unwrap_or_else(|| PathBuf::from("d:\\discord custom\\debug.log"));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", msg);
        let _ = f.flush();
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        log_debug(&format!("[PANIC] {:?}", info));
    }));

    if let Err(e) = run_app() {
        log_debug(&format!("[MAIN_ERR] {:?}", e));
    }
}

fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    log_debug(&format!("\n=== Customcord process started at {:?} ===", std::time::SystemTime::now()));

    let event_loop: EventLoop<AppCmd> = EventLoopBuilder::<AppCmd>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Customcord")
        .with_decorations(false)
        .with_theme(Some(tao::window::Theme::Dark))
        .with_inner_size(LogicalSize::new(1280.0, 780.0))
        .with_min_inner_size(LogicalSize::new(800.0, 500.0))
        .build(&event_loop)?;

    #[cfg(target_os = "windows")]
    {
        let hwnd = window.hwnd() as isize;
        unsafe {
            unsafe extern "system" {
                fn GetWindowLongW(hWnd: isize, nIndex: i32) -> i32;
                fn SetWindowLongW(hWnd: isize, nIndex: i32, dwNewLong: i32) -> i32;
                fn SetWindowPos(
                    hWnd: isize,
                    hWndInsertAfter: isize,
                    X: i32,
                    Y: i32,
                    cx: i32,
                    cy: i32,
                    uFlags: u32,
                ) -> i32;
            }
            let style = GetWindowLongW(hwnd, -16); // GWL_STYLE
            // WS_THICKFRAME (0x00040000) | WS_MINIMIZEBOX (0x00020000) | WS_MAXIMIZEBOX (0x00010000)
            SetWindowLongW(hwnd, -16, style | 0x00040000 | 0x00020000 | 0x00010000);
            SetWindowPos(hwnd, 0, 0, 0, 0, 0, 0x0002 | 0x0001 | 0x0004 | 0x0020);

            unsafe extern "system" {
                fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                fn LoadIconW(hInstance: isize, lpIconName: *const u16) -> isize;
                fn SendMessageW(hWnd: isize, Msg: u32, wParam: usize, lParam: isize) -> isize;
            }
            let h_instance = GetModuleHandleW(std::ptr::null());
            let h_icon = LoadIconW(h_instance, 1 as *const u16);
            if h_icon != 0 {
                SendMessageW(hwnd, 0x0080, 0, h_icon); // WM_SETICON, ICON_SMALL
                SendMessageW(hwnd, 0x0080, 1, h_icon); // WM_SETICON, ICON_BIG
            }
        }
    }

    let appdata = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let data_dir = appdata.join("Customcord");

    let mut web_context = WebContext::new(Some(data_dir));

    let mut full_script = String::with_capacity(
        VENCORD_BUNDLE.len() + VENCORD_POLYFILL.len() + CUSTOMCORD_INJECTION_SCRIPT.len() + 2048,
    );
    full_script.push_str(CUSTOMCORD_INJECTION_SCRIPT);
    full_script.push_str("\n;\n");
    full_script.push_str(VENCORD_POLYFILL);
    full_script.push_str("\n;\ntry {\n");
    full_script.push_str(VENCORD_BUNDLE);
    full_script.push_str("\n} catch (err) { if (window.ipc) window.ipc.postMessage('log: VENCORD_CRASH: ' + (err.stack || err)); }\n");

    let ptt_vk = Arc::new(AtomicI32::new(20)); // CapsLock по умолчанию
    let ptt_vk_ipc = Arc::clone(&ptt_vk);
    let ipc_proxy = proxy.clone();
    let webview = WebViewBuilder::with_web_context(&mut web_context)
        .with_user_agent(BROWSER_USER_AGENT)
        .with_additional_browser_args(CHROMIUM_ADDITIONAL_ARGS)
        .with_background_color((49, 51, 56, 255))
        .with_browser_accelerator_keys(false)
        .with_devtools(true)
        .with_clipboard(true)
        .with_autoplay(true)
        .with_initialization_script(&full_script)
        .with_on_page_load_handler(|event, url| {
            let ev_str = match event {
                wry::PageLoadEvent::Started => "Started",
                wry::PageLoadEvent::Finished => "Finished",
            };
            log_debug(&format!("[PAGE_LOAD] {}: {}", ev_str, url));
        })
        .with_ipc_handler(move |req| {
            let body = req.body();
            log_debug(&format!("[IPC] {}", body));
            if body.starts_with("ptt_key:") {
                let parts: Vec<&str> = body.trim_start_matches("ptt_key:").split(':').collect();
                if parts.len() >= 2 {
                    let dev_type: i32 = parts[0].parse().unwrap_or(0);
                    let code: i32 = parts[1].parse().unwrap_or(20);
                    let vk = get_vk_from_shortcut(dev_type, code);
                    ptt_vk_ipc.store(vk, Ordering::Relaxed);
                }
            } else if body.starts_with("zoom:") {
                let factor_str = body.trim_start_matches("zoom:");
                if let Ok(factor) = factor_str.parse::<f64>() {
                    let scale = factor / 100.0;
                    let _ = ipc_proxy.send_event(AppCmd::Zoom(scale));
                }
            } else {
                match body.as_str() {
                    "minimize" => { let _ = ipc_proxy.send_event(AppCmd::Minimize); }
                    "maximize" => { let _ = ipc_proxy.send_event(AppCmd::Maximize); }
                    "close" => { let _ = ipc_proxy.send_event(AppCmd::Close); }
                    "drag" => { let _ = ipc_proxy.send_event(AppCmd::Drag); }
                    _ => {}
                }
            }
        })
        .with_url(DISCORD_URL)
        .build(&window)?;

    let is_running = Arc::new(AtomicBool::new(true));
    start_background_optimizer(Arc::clone(&is_running));
    start_global_ptt_listener(Arc::clone(&is_running), Arc::clone(&ptt_vk), proxy.clone());

    event_loop.run(move |event, _, control_flow| {
        let _ = &webview;
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(AppCmd::Minimize) => {
                window.set_minimized(true);
                trim_all_memory();
            }
            Event::UserEvent(AppCmd::Maximize) => {
                window.set_maximized(!window.is_maximized());
            }
            Event::UserEvent(AppCmd::Close) => {
                is_running.store(false, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(AppCmd::Drag) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(AppCmd::PttDown) => {
                let _ = webview.evaluate_script("if (window.__customcord_ptt) window.__customcord_ptt(true);");
            }
            Event::UserEvent(AppCmd::PttUp) => {
                let _ = webview.evaluate_script("if (window.__customcord_ptt) window.__customcord_ptt(false);");
            }
            Event::UserEvent(AppCmd::Zoom(scale)) => {
                let _ = webview.zoom(scale);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                is_running.store(false, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
            _ => (),
        }
    });

    #[allow(unreachable_code)]
    Ok(())
}
