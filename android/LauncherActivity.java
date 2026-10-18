// SwitchRecomp Android launcher. Copyright 2026 MZ Labs; fork additions
// maintained by katakishi0. Apache-2.0, see LICENSE.txt and NOTICE.txt.
// Uses only Android platform widgets: no Gradle, proprietary assets or permissions.
package org.switchrecomp.frontend;

import android.app.Activity;
import android.app.NativeActivity;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.SeekBar;
import android.widget.TextView;
import android.widget.Toast;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.LinkedHashMap;
import java.util.Map;

public final class LauncherActivity extends Activity {
    private static final int EXPORT_LOG = 43;
    private final int BG = Color.rgb(13, 18, 22);
    private final int FG = Color.rgb(225, 238, 237);
    private final int ACCENT = Color.rgb(83, 210, 185);
    private final int MUTED = Color.rgb(165, 184, 187);
    private final String game = "game";
    private LinearLayout body;
    private TextView status;
    private TextView preview;
    private SharedPreferences prefs;

    private int dp(int d) { return (int) (d * getResources().getDisplayMetrics().density + .5f); }
    private TextView text(String value, int size, int color) {
        TextView t = new TextView(this);
        t.setText(value);
        t.setTextSize(size);
        t.setTextColor(color);
        t.setPadding(0, dp(6), 0, dp(6));
        return t;
    }
    private void heading(String value) {
        TextView t = text(value, 18, FG);
        t.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        t.setPadding(0, dp(14), 0, dp(4));
        body.addView(t);
    }
    private Button button(String value, View.OnClickListener action) {
        Button b = new Button(this);
        b.setText(value);
        b.setAllCaps(false);
        b.setOnClickListener(action);
        body.addView(b, new LinearLayout.LayoutParams(-1, dp(54)));
        return b;
    }
    private CheckBox check(String title, String key, boolean initial) {
        CheckBox b = new CheckBox(this);
        b.setText(title);
        b.setTextColor(FG);
        b.setButtonTintList(android.content.res.ColorStateList.valueOf(ACCENT));
        b.setChecked(prefs.getBoolean(key, initial));
        b.setOnCheckedChangeListener((v, checked) -> prefs.edit().putBoolean(key, checked).apply());
        body.addView(b);
        return b;
    }
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        prefs = getSharedPreferences("switchrecomp", MODE_PRIVATE);
        getWindow().setStatusBarColor(BG);
        getWindow().setNavigationBarColor(BG);
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(BG);
        body = new LinearLayout(this);
        body.setOrientation(LinearLayout.VERTICAL);
        body.setPadding(dp(24), dp(18), dp(24), dp(32));
        scroll.addView(body);
        setContentView(scroll);

        TextView title = text("SwitchRecomp  |  Android", 24, ACCENT);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        body.addView(title);
        body.addView(text("Experimental native recompilation port", 13, MUTED));
        heading("Game");
        body.addView(text(getApplicationInfo().loadLabel(getPackageManager()).toString(), 18, FG));
        body.addView(text("This APK contains code recompiled for one title. It cannot execute a different NSP or XCI.", 13, MUTED));
        button("Start game", v -> startGame());
        heading("Graphics");
        body.addView(text("OpenGL ES 3.2 (current renderer). Vulkan is not implemented for this APK.", 14, MUTED));
        check("Wait for VSync", "vsync", false);
        heading("Controls");
        check("Show on-screen touch controls", "touch", true);
        body.addView(text("Touch-control opacity", 14, MUTED));
        SeekBar opacity = new SeekBar(this);
        opacity.setMax(80);
        opacity.setProgress(prefs.getInt("opacity", 50));
        opacity.setProgressTintList(android.content.res.ColorStateList.valueOf(ACCENT));
        opacity.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener() {
            public void onProgressChanged(SeekBar s, int progress, boolean user) {
                if (user) prefs.edit().putInt("opacity", progress).apply();
            }
            public void onStartTrackingTouch(SeekBar s) {}
            public void onStopTrackingTouch(SeekBar s) {}
        });
        body.addView(opacity);
        heading("Diagnostics");
        check("Verbose logs (can reduce performance)", "verbose", true);
        check("Automatic render-target diagnostic dumps", "diag", false);
        check("CPU profiling (extra overhead)", "profile", false);
        status = text("", 13, MUTED);
        body.addView(status);
        button("Refresh recent log", v -> refreshLog());
        button("Save log to a file", v -> saveLog());
        preview = text("", 12, FG);
        preview.setTypeface(Typeface.MONOSPACE);
        preview.setTextIsSelectable(true);
        body.addView(preview);
        body.addView(text("For Android ANR and native crashes, additionally collect adb logcat -v threadtime. The in-app game.log cannot capture the system ANR stack trace.", 12, MUTED));
        refreshLog();
    }
    private File gameIni() { return new File(getFilesDir(), game + ".ini"); }
    private File gameLog() { return new File(getFilesDir(), game + ".log"); }

    // Preserve unrelated advanced settings on every launch. The native runtime
    // reads this same INI and reloads it, so the game and frontend agree.
    private void writeSettings() throws Exception {
        LinkedHashMap<String, String> options = new LinkedHashMap<>();
        options.put("forced_vsync", prefs.getBoolean("vsync", false) ? "1" : "0");
        options.put("touch_controls", prefs.getBoolean("touch", true) ? "1" : "0");
        options.put("touch_controls_opacity", String.format(java.util.Locale.US, "%.2f",
                (prefs.getInt("opacity", 50) + 20) / 100.f));
        options.put("log", prefs.getBoolean("verbose", true) ? "2" : "1");
        options.put("diag", prefs.getBoolean("diag", false) ? "1" : "0");
        options.put("profile", prefs.getBoolean("profile", false) ? "1" : "0");
        File f = gameIni();
        String old = f.isFile() ? new String(Files.readAllBytes(f.toPath()), StandardCharsets.UTF_8) : "";
        StringBuilder updated = new StringBuilder();
        for (String line : old.split("\n", -1)) {
            String trimmed = line.trim();
            int eq = trimmed.indexOf('=');
            if (eq > 0 && !trimmed.startsWith(";") && !trimmed.startsWith("#")) {
                String key = trimmed.substring(0, eq).trim();
                if (options.containsKey(key)) {
                    updated.append(key).append('=').append(options.remove(key)).append('\n');
                    continue;
                }
            }
            updated.append(line).append('\n');
        }
        for (Map.Entry<String, String> pair : options.entrySet())
            updated.append(pair.getKey()).append('=').append(pair.getValue()).append('\n');
        File tmp = new File(getFilesDir(), game + ".ini.tmp");
        Files.write(tmp.toPath(), updated.toString().getBytes(StandardCharsets.UTF_8));
        if (!tmp.renameTo(f)) {
            Files.write(f.toPath(), updated.toString().getBytes(StandardCharsets.UTF_8));
            tmp.delete();
        }
    }
    private void startGame() {
        try {
            writeSettings();
            startActivity(new Intent(this, NativeActivity.class));
        } catch (Exception e) {
            Toast.makeText(this, "Cannot launch: " + e.getMessage(), Toast.LENGTH_LONG).show();
        }
    }
    private void refreshLog() {
        File f = gameLog();
        if (!f.isFile()) {
            status.setText("No game log yet. Launch the game first.");
            preview.setText("");
            return;
        }
        status.setText("Saved: " + f.length() / 1024 + " KiB — " + f.getAbsolutePath());
        try (RandomAccessFile data = new RandomAccessFile(f, "r")) {
            int length = (int) Math.min(12000, data.length());
            data.seek(data.length() - length);
            byte[] tail = new byte[length];
            data.readFully(tail);
            preview.setText(new String(tail, StandardCharsets.UTF_8));
        } catch (Exception e) {
            preview.setText("Could not read log: " + e.getMessage());
        }
    }
    private void saveLog() {
        if (!gameLog().isFile()) {
            Toast.makeText(this, "Run the game to create a log first.", Toast.LENGTH_SHORT).show();
            return;
        }
        Intent pick = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        pick.setType("text/plain");
        pick.addCategory(Intent.CATEGORY_OPENABLE);
        pick.putExtra(Intent.EXTRA_TITLE, "SwitchRecomp-game.log");
        startActivityForResult(pick, EXPORT_LOG);
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != EXPORT_LOG || result != RESULT_OK || data == null || data.getData() == null)
            return;
        try (InputStream src = Files.newInputStream(gameLog().toPath());
             OutputStream dst = getContentResolver().openOutputStream(data.getData(), "w")) {
            if (dst == null) throw new java.io.IOException("No output stream");
            byte[] buf = new byte[65536];
            int n;
            while ((n = src.read(buf)) != -1) dst.write(buf, 0, n);
            Toast.makeText(this, "Log saved.", Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(this, "Log export failed: " + e.getMessage(), Toast.LENGTH_LONG).show();
        }
    }
    @Override protected void onResume() {
        super.onResume();
        if (status != null) refreshLog();
    }
}
