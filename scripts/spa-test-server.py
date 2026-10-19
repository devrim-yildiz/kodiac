#!/usr/bin/env python3
"""Codex, 2026-10-21. Isolated real-server browser fixture; no production data."""
import json
import http.server
import threading
import os
from pathlib import Path
import signal
import sqlite3
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get('SPA_BINARY', ROOT / 'target/release/medienserver'))
PORT = int(os.environ.get('SPA_TEST_PORT', '18197'))


def artwork(path, title, color, landscape=False):
    width, height = (1600, 900) if landscape else (600, 900)
    path.write_text(f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<defs><linearGradient id="sky" x2="0.4" y2="1"><stop stop-color="{color}"/><stop offset="1" stop-color="#111a25"/></linearGradient><linearGradient id="fog" x2="0" y2="1"><stop stop-color="#d5ddd0" stop-opacity=".1"/><stop offset="1" stop-color="#142027"/></linearGradient></defs>
<rect width="100%" height="100%" fill="url(#sky)"/><circle cx="{width*.72}" cy="{height*.27}" r="{width*.16}" fill="#f6ebcd" opacity=".6"/>
<path d="M0 {height*.7} L{width*.22} {height*.38} L{width*.43} {height*.68} L{width*.66} {height*.45} L{width} {height*.72} V{height} H0Z" fill="#172a32"/>
<path d="M0 {height*.8} L{width*.29} {height*.62} L{width*.55} {height*.84} L{width*.8} {height*.61} L{width} {height*.8} V{height} H0Z" fill="url(#fog)"/>
<path d="M{width*.49} {height*.71}v{height*.2}" stroke="#071318" stroke-width="{width*.012}"/><circle cx="{width*.49}" cy="{height*.7}" r="{width*.012}" fill="#071318"/>
<text x="{width*.08}" y="{height*.91}" font-family="sans-serif" font-weight="600" font-size="{width*.075}" fill="#ebe8d9" letter-spacing="{width*.007}">{title.upper()}</text></svg>''')


def main():
    with tempfile.TemporaryDirectory(prefix='medienserver-spa-fixture-') as directory:
        root = Path(directory)
        media = root / 'media'
        media.mkdir()
        password = root / 'password'
        password.write_text('ui-test-password-only')
        password.chmod(0o600)
        film = media / 'Afterlight.mp4'
        subprocess.run(['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=480x270:rate=24', '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-f', 'lavfi', '-i', 'sine=frequency=880:sample_rate=48000', '-map', '0:v', '-map', '1:a', '-map', '2:a', '-t', '90', '-c:v', 'libx264', '-preset', 'ultrafast', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-movflags', '+faststart', str(film)], check=True)
        sub = media / 'Afterlight.eng.srt'
        sub.write_text('1\n00:00:00,000 --> 00:01:20,000\nA story worth staying for.\n')
        for name, color in [('afterlight', '#667c86'), ('northstar', '#3b6b76'), ('signal', '#89554d'), ('orbit', '#4d5c89'), ('still', '#3a7272')]:
            artwork(media / f'{name}.svg', name, color)
            artwork(media / f'{name}-wide.svg', name, color, True)
        db = root / 'catalog.db'
        env = {k: v for k, v in os.environ.items() if not k.startswith(('TMDB_', 'ADMIN_'))}
        env['RUST_LOG'] = 'medienserver=info'
        command = [str(BINARY), '--media-path', str(media), '--db-path', str(db), '--metadata-path', str(root / 'metadata'), '--admin-password-file', str(password), '--scan-interval-seconds', '0', '--hw-accel', 'software', '--port', str(PORT)]
        with (root / 'server.log').open('w+') as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
            def stop(*_):
                if process.poll() is None: process.send_signal(signal.SIGTERM)
            signal.signal(signal.SIGTERM, stop)
            signal.signal(signal.SIGINT, stop)
            try:
                deadline = time.monotonic() + 25
                while True:
                    if process.poll() is not None: raise RuntimeError('Fixture server exited')
                    try:
                        with urllib.request.urlopen(f'http://127.0.0.1:{PORT}/api/v1/health', timeout=1): pass
                        conn = sqlite3.connect(db)
                        scanned = conn.execute('SELECT count(*) FROM libraries WHERE last_scan_at IS NOT NULL').fetchone()[0]
                        if scanned: break
                        conn.close()
                    except (OSError, sqlite3.Error): pass
                    if time.monotonic() > deadline: raise RuntimeError('Fixture initialization timed out')
                    time.sleep(.1)
                streams = json.dumps([
                    {'index': 0, 'codec_type': 'video', 'codec_name': 'h264', 'width': 3840, 'height': 2160, 'color_transfer': 'smpte2084'},
                    {'index': 1, 'codec_type': 'audio', 'codec_name': 'aac', 'channels': 2, 'tags': {'language': 'eng'}, 'disposition': {'default': 1}},
                    {'index': 2, 'codec_type': 'audio', 'codec_name': 'aac', 'channels': 2, 'tags': {'language': 'deu'}},
                    {'index': 3, 'codec_type': 'subtitle', 'codec_name': 'subrip', 'external_path': str(sub), 'tags': {'language': 'eng'}},
                ])
                conn.execute('PRAGMA foreign_keys=ON')
                conn.execute('DELETE FROM media_items')
                library = conn.execute('SELECT id FROM libraries LIMIT 1').fetchone()[0]
                plot = 'At the edge of a changing world, a quiet discovery sets an extraordinary journey in motion. Some stories stay with you long after the screen goes dark.'
                def item(identifier, title, art, path, series=None, season=None, episode=None):
                    conn.execute('INSERT INTO media_items(id,library_id,path,kind,title,plot,year,runtime_seconds,series_name,season,episode,poster_path,backdrop_path,container,video_codec,audio_codec,streams_json,size_bytes,modified_ns,scan_generation,probe_version) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)',
                        (identifier, library, str(path), 'episode' if series else 'movie', title, plot, 2026, 90, series, season, episode, str(media / f'{art}.svg'), str(media / f'{art}-wide.svg'), 'mp4', 'h264', 'aac', streams, film.stat().st_size, 'fixture', 'fixture', 1))
                for n in range(5200):
                    title = ['Afterlight', 'A Quiet Orbit', 'Before the Dawn'][n] if n < 3 else f'Library Film {n:04}'
                    item(f'movie-{n:04}', title, ['afterlight', 'orbit', 'signal', 'northstar'][n % 4], film if n == 0 else media / f'fixture-film-{n}.mp4')
                for show, art in [('Northstar', 'northstar'), ('The Signal', 'signal')]:
                    for season in [1, 2]:
                        for episode in [1, 2, 3]:
                            path = media / f'{show}.S{season:02}E{episode:02}.mp4'
                            os.link(film, path)
                            item(f'{art}-{season}-{episode}', ['The Arrival', 'An Open Door', 'A Distant Light'][episode - 1], art, path, show, season, episode)
                genres = json.dumps([{'id': 878, 'name': 'Science Fiction'}, {'id': 18, 'name': 'Drama'}])
                conn.execute('UPDATE movies SET genres_json=?', (genres,))
                conn.execute('UPDATE shows SET overview=?,genres_json=?', (plot, genres))
                conn.execute("INSERT INTO profiles(id,user_id,name) SELECT 'guests',id,'Guests' FROM users LIMIT 1")
                conn.execute("INSERT INTO playback_history(profile_id,media_item_id,position_ticks) SELECT id,'movie-0000',120000000 FROM profiles WHERE is_default=1")
                conn.execute("INSERT INTO playback_history(profile_id,media_item_id,position_ticks) VALUES('guests','northstar-1-1',180000000)")
                conn.execute("UPDATE media_items SET added_at=1000")
                conn.execute("UPDATE media_items SET added_at=2000 WHERE id IN('movie-0000','movie-0001','movie-0002','movie-0003','movie-0004')")
                conn.execute("UPDATE media_items SET rating=8.4 WHERE id='movie-0000'")
                conn.execute("UPDATE media_items SET rating=8.1 WHERE id='movie-0001'")
                conn.execute("INSERT INTO collections(id,title) VALUES('afterlight','The Afterlight Collection')")
                conn.execute("INSERT INTO collection_movies VALUES('afterlight','movie-0000'),('afterlight','movie-0001')")
                conn.execute("INSERT INTO people(name,bio) VALUES('Mira Chen','A filmmaker exploring the quiet spaces between science and memory.')")
                conn.execute("INSERT INTO item_people VALUES('movie-0000','Mira Chen','Director')")
                conn.execute("UPDATE media_items SET chapters_json=? WHERE id='northstar-1-1'",(json.dumps([{'start_time':'0','end_time':'8','tags':{'title':'Intro'}}]),))
                os.link(film,media / 'Afterlight.trailer.mp4')
                subprocess.run(['ffmpeg','-v','error','-f','lavfi','-i','sine=frequency=220:duration=4','-q:a','5',str(media / 'theme.mp3')],check=True)
                conn.commit(); conn.close()
                class Ready(http.server.BaseHTTPRequestHandler):
                    def do_GET(self):
                        self.send_response(200); self.end_headers(); self.wfile.write(b'ready')
                    def log_message(self, *_): pass
                readiness=http.server.ThreadingHTTPServer(('127.0.0.1',PORT+1),Ready)
                threading.Thread(target=readiness.serve_forever,daemon=True).start()
                print(f'SPA fixture ready at http://127.0.0.1:{PORT} (5,200 films + 2 shows).', flush=True)
                process.wait()
            finally:
                stop()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait()

if __name__ == '__main__':
    main()
