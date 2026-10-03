package app.knit;
import android.content.*;

public final class BootReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context,Intent intent) { ConnectionService.autoStart(context); }
}
