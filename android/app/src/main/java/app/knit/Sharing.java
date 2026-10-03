package app.knit;
import android.content.*;
import android.net.Uri;
import android.os.*;
import android.provider.MediaStore;
import android.graphics.Bitmap;
import android.webkit.MimeTypeMap;
import org.json.*;
import java.io.*;
import java.nio.*;
final class Sharing {
    static volatile String pendingText;
    static volatile Uri pendingUri;
    static volatile String pendingUrl;
    static volatile String lastReceived="";
    private static final Handler ui=new Handler(Looper.getMainLooper());
    static void receiveText(Context c,String text) {pendingText=text;pendingUri=null;applyPending(c);}
    static void receiveUrl(Context c,String url) {
        try { Uri u=Uri.parse(url);if(url.length()>8192 || !("https".equals(u.getScheme())||"http".equals(u.getScheme())) || u.getHost()==null || u.getUserInfo()!=null)return;pendingUrl=url;ConnectionService.report("URLを受信しました。Knitアプリから開けます。"); }
        catch(Exception ignored){}
    }
    static void applyPending(Context c) {
        if(!ConnectionService.clip || !(KnitIme.selected(c)||MainActivity.foreground))return;
        ClipboardManager clipboard=(ClipboardManager)c.getSystemService(Context.CLIPBOARD_SERVICE);
        if(pendingText!=null) {String value=pendingText;pendingText=null;lastReceived=value;clipboard.setPrimaryClip(ClipData.newPlainText("Knit",value));}
        else if(pendingUri!=null) {Uri value=pendingUri;pendingUri=null;clipboard.setPrimaryClip(ClipData.newUri(c.getContentResolver(),"Knit image",value));}
    }
    static void sendClipboard(Context c) {
        if(!ConnectionService.connected||!ConnectionService.clip)return;
        ClipboardManager manager=(ClipboardManager)c.getSystemService(Context.CLIPBOARD_SERVICE);
        ClipData clip=manager.getPrimaryClip();if(clip==null || clip.getItemCount()==0)return;
        if(Build.VERSION.SDK_INT>=33 && clip.getDescription().getExtras()!=null && clip.getDescription().getExtras().getBoolean(ClipDescription.EXTRA_IS_SENSITIVE))return;
        CharSequence text=clip.getItemAt(0).getText();if(text==null)return;
        String value=text.toString();if(value.length()>1024*1024||value.equals(lastReceived))return;
        ConnectionService.send(Native.obj("t","clip","text",value));lastReceived=value;
    }
    static void receiveFiles(Context c,JSONObject event) throws Exception {
        JSONArray files=event.optJSONArray("files");
        if(files!=null) {
            for(int i=0;i<files.length();i++){File f=new File(files.getString(i));publish(c,f,false);if(!f.delete())f.deleteOnExit();}
            ConnectionService.report("Downloads/Knitへファイルを保存しました。");
        } else if(event.has("image")) {
            File dib=new File(event.getString("image"));File png=new File(c.getCacheDir(),"clipboard-"+System.currentTimeMillis()+".png");
            Bitmap bitmap=decodeDib(dib);
            try(FileOutputStream out=new FileOutputStream(png)){if(!bitmap.compress(Bitmap.CompressFormat.PNG,100,out))throw new IOException();}finally{bitmap.recycle();dib.delete();}
            Uri uri=publish(c,png,true);png.delete();
            ui.post(()-> {pendingUri=uri;pendingText=null;applyPending(c);ConnectionService.report("画像をPictures/Knitへ保存しました。");});
        }
    }
    private static Uri publish(Context c,File file,boolean image) throws Exception {
        ContentValues values=new ContentValues();values.put(MediaStore.MediaColumns.DISPLAY_NAME,file.getName());
        String ext=MimeTypeMap.getFileExtensionFromUrl(file.getName()).toLowerCase(java.util.Locale.ROOT);
        String mime=image?"image/png":MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);
        values.put(MediaStore.MediaColumns.MIME_TYPE,mime==null?"application/octet-stream":mime);
        values.put(MediaStore.MediaColumns.RELATIVE_PATH,image?"Pictures/Knit":"Download/Knit");values.put(MediaStore.MediaColumns.IS_PENDING,1);
        Uri uri=c.getContentResolver().insert(image?MediaStore.Images.Media.EXTERNAL_CONTENT_URI:MediaStore.Downloads.EXTERNAL_CONTENT_URI,values);
        if(uri==null)throw new IOException("保存先を作成できませんでした。");
        try(InputStream in=new FileInputStream(file);OutputStream out=c.getContentResolver().openOutputStream(uri)) {
            if(out==null)throw new IOException();byte[] bytes=new byte[65536];int n;while((n=in.read(bytes))>=0)out.write(bytes,0,n);
        } catch(Exception e){c.getContentResolver().delete(uri,null,null);throw e;}
        values.clear();values.put(MediaStore.MediaColumns.IS_PENDING,0);c.getContentResolver().update(uri,values,null,null);return uri;
    }
    private static Bitmap decodeDib(File file) throws Exception {
        if(file.length()<40 || file.length()>64L*1024*1024)throw new IOException("画像サイズが不正です。");
        byte[] bytes;
        try(java.io.DataInputStream in=new java.io.DataInputStream(new FileInputStream(file))){bytes=new byte[(int)file.length()];in.readFully(bytes);}
        ByteBuffer b=ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
        int header=b.getInt(0),width=b.getInt(4),height=b.getInt(8),bits=b.getShort(14)&65535,compression=b.getInt(16);
        if(header<40||header>bytes.length||width<=0||height==0||height==Integer.MIN_VALUE||(bits!=24&&bits!=32)||compression!=0)throw new IOException("この画像形式は対応していません。");
        int h=Math.abs(height);long row=(((long)width*bits+31)/32)*4;
        if((long)width*h>16*1024*1024 || header+row*h>bytes.length)throw new IOException("画像が大きすぎるか、破損しています。");
        int[] pixels=new int[width*h];
        for(int y=0;y<h;y++)for(int x=0;x<width;x++){int pos=(int)(header+(height>0?h-1-y:y)*row)+(x*bits/8);pixels[y*width+x]=0xff000000|((bytes[pos+2]&255)<<16)|((bytes[pos+1]&255)<<8)|(bytes[pos]&255);}
        return Bitmap.createBitmap(pixels,width,h,Bitmap.Config.ARGB_8888);
    }
    static File prepareUpload(Context c,Uri uri) throws Exception {
        File directory=new File(c.getCacheDir(),"send");directory.mkdirs();
        String name="shared-file";
        try(android.database.Cursor cursor=c.getContentResolver().query(uri,new String[]{android.provider.OpenableColumns.DISPLAY_NAME},null,null,null)) {if(cursor!=null&&cursor.moveToFirst())name=cursor.getString(0);}
        name=name==null?"shared-file":name.replaceAll("[\\\\/\\p{Cntrl}]","_");if(name.isBlank()||name.equals(".")||name.equals(".."))name="shared-file";
        File dir=new File(directory,java.util.UUID.randomUUID().toString());dir.mkdirs();File file=new File(dir,name);
        try(InputStream in=c.getContentResolver().openInputStream(uri);OutputStream out=new FileOutputStream(file)) {
            if(in==null)throw new IOException();long total=0;int n;byte[] buffer=new byte[65536];
            while((n=in.read(buffer))>=0){total+=n;if(total>256L*1024*1024)throw new IOException("この試作版の送信上限は256MiBです。");out.write(buffer,0,n);}
        }catch(Exception e){file.delete();dir.delete();throw e;}return file;
    }
}
