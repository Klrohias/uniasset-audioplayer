using Uniasset.AudioPlayer;
using UnityEngine;

namespace UniassetAudioPlayerDemo
{
    /// <summary>
    /// Simple test UI using IMGUI. Shows Play / Pause / Resume / Stop buttons
    /// to exercise the AudioPlayer API.
    /// </summary>
    public class TestUIManager : MonoBehaviour
    {
        [SerializeField]
        private AudioClip _clip;

        private AudioPlayer _player;
        private PlayHandle _handle;
        private bool _handleActive;

        private void OnGUI()
        {
            GUILayout.BeginArea(new Rect(16, 16, 240, 200));
            GUILayout.BeginVertical("box");

            GUILayout.Label("Uniasset AudioPlayer Demo", GUILayout.ExpandWidth(true));

            GUILayout.Space(8);

            // ---- Play ----
            GUI.enabled = _clip != null && _player == null;
            if (GUILayout.Button("Play", GUILayout.Height(36)))
            {
                Play();
            }

            // ---- Pause ----
            GUI.enabled = _handleActive;
            if (GUILayout.Button("Pause", GUILayout.Height(36)))
            {
                _handle?.Pause();
            }

            // ---- Resume ----
            GUI.enabled = _handleActive && (_handle?.IsPaused ?? false);
            if (GUILayout.Button("Resume", GUILayout.Height(36)))
            {
                _handle?.Resume();
            }

            // ---- Stop ----
            GUI.enabled = _player != null;
            if (GUILayout.Button("Stop", GUILayout.Height(36)))
            {
                Stop();
            }

            GUI.enabled = true;

            // Status line
            GUILayout.Space(8);
            var status = "Idle";
            if (_player != null && _handleActive && (_handle?.IsAlive ?? false))
                status = _handle?.IsPaused == true ? "Paused" : "Playing";
            else if (_player != null)
                status = "Stopped";
            GUILayout.Label($"Status: {status}");

            GUILayout.EndVertical();
            GUILayout.EndArea();
        }

        private void Play()
        {
            Stop();

            _player = new AudioPlayer();
            _handle = _player.Play(_clip);
            _handleActive = true;
        }

        private void Stop()
        {
            _handleActive = false;
            _handle?.Dispose();
            _handle = null;
            _player?.Dispose();
            _player = null;
        }

        private void Update()
        {
            // Periodic EOF cleanup.
            if (_player != null)
            {
                _player.CleanupEof();

                // If the stream finished, clean up.
                if (_handleActive && !(_handle?.IsAlive ?? false))
                {
                    _handleActive = false;
                    _handle?.Dispose();
                    _handle = null;
                }
            }
        }

        private void OnDestroy()
        {
            Stop();
        }
    }
}
