package jLib;
import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.Reader;
import java.io.StringWriter;
import java.net.FileNameMap;
import java.net.URLConnection;
import java.nio.CharBuffer;
import java.nio.charset.Charset;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.Arrays;
import java.util.Enumeration;
import java.util.Iterator;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;


/**
 * File operations library - lightweight utilities for file system operations
 */
public class LibFile {


    public static String file2string( File file ) throws IOException {
        return file2string( file, null );
    }
    @SuppressWarnings("UseSpecificCatch")
    public static String file2string( File file, Charset charset ) throws IOException {
        char[] cArr = new char[8192];
        FileInputStream fInp = null;
        InputStreamReader rInp = null;
        BufferedReader bInp = null;
        StringWriter sOut = new StringWriter();
        try {
            fInp = new FileInputStream( file );
            rInp = ( charset==null
                ? new InputStreamReader( fInp )
                : new InputStreamReader( fInp, charset )
            );
            bInp = new BufferedReader( rInp );
            while ( true ) {
                int readCount = bInp.read( cArr );
                if ( readCount<0 ) break;
                if ( readCount==0 ) {
                    try { Thread.sleep( 1 ); } catch ( InterruptedException ignore ) {}
                } else {
                    sOut.append( CharBuffer.wrap( cArr, 0, readCount ) );
                }
            }
        } finally {
            try { if(bInp!=null)bInp.close(); } catch ( Throwable ignore ) {}
            try { if(rInp!=null)rInp.close(); } catch ( Throwable ignore ) {}
            try { if(fInp!=null)fInp.close(); } catch ( Throwable ignore ) {}
        }
        return sOut.toString();
    }
    @SuppressWarnings("unused")
    private static boolean file2string_TEST_( boolean findLineNumber ) throws Exception {
        if ( findLineNumber ) throw new RuntimeException();
        File f = new File( "log/file2string_TEST_"+Lib.timeStamp() );
        f.createNewFile();
        String s = file2string( f );
        LibTest.asrtEQ( s, "" );
        string2file( "hello world", f, false );
        s = file2string( f );
        LibTest.asrtEQ( s, "hello world" );
        f.delete();
        return true;
    }



    public static boolean string2file( Object str, File file, boolean append ) {
        return string2file( str, file, append, null );
    }
    public static boolean string2file( Object str, File file, boolean append, Charset charset ) {
        File parent = file.getParentFile();
        if ( parent!=null && !parent.exists() ) parent.mkdirs();
        if ( str==null ) str = "";
        try (
            FileOutputStream fOut = new FileOutputStream( file, append );
            OutputStreamWriter wOut = (
                charset==null ? new OutputStreamWriter( fOut ) : new OutputStreamWriter( fOut, charset )
            );
            BufferedWriter bOut = new BufferedWriter( wOut );
        ) {
            bOut.write( str.toString() );
            return true;
        } catch ( IOException ioe ) { return false; }
    }



    public static boolean fileCopy( Object src, Object dst ) {
        return fileCopy( src, dst, false, false, null );
    }
    /**
    * Copies or moves or deletes a file or subdirectory.
    *
    * - Creates as many levels of subdirectories as needed.
    *
    * - While operating on a subdirectory, if some part of the operation fails,
    *	 the rest of the operation will normally continue.
    *
    * - Preserves modified date.
    *
    * - Can move or copy a parent directory into its own child directory,
    *	 or a child directory into its own parent.
    *
    * Examples:
    *
    * // Make D:\logs.bak indentical to C:/WINDOWS/system32/LogFiles
    * fileCopy( "c:/WINDOWS/system32/LogFiles", "d:/logs.bak", false, true );
    *
    * // Copy c:\windows\system.ini into d:/logs.bak directory
    * fileCopy( "c:/WINDOWS/system.ini", "d:/logs.bak", false, true );
    *
    * // Move D:\logs.bak to C:\logs.COPY
    * fileCopy( "D:\\logs.bak", "c:/logs.COPY", true, true );
    *
    * // Move C:\logs.COPY into C:\logs.COPY\copy\copy2\copy3
    * fileCopy( "C:/logs.COPY", "C:/logs.COPY/copy", true, false );
    *
    * // Rename C:/logs.COPY to C:/logs.bak
    * fileCopy( "C:/logs.COPY", "C:/logs.bak", true, true );
    *
    * // Remove C:\logs.bak and everything under it
    * fileCopy( "C:/logs.bak", null, true, false );
    *
    * @param srcFile
    *				file name or File.
    * @param dstFile
    *				file name or File.
    * @param move
    *				Attempts to move or rename, attempts to delete srcFile.
    *				Will attempt copy if more or rename fails.
    * @param overwrite
    *				If dstFile exists and is not a directory, will delete before
    *				moving or copying to it.
    * @param filter
    *				If null, no filtering is done. Filter's equals method is called
    *        before each copy, move or delete. If false is returned, this
    *        item is skipped. Argument passed to filter's equals method is
    *        an array that looks like:
    *        new Object[]{ "moving", new File(src), new File(dst) }
    * @return true if completely successful
    **/
    @SuppressWarnings({"UseSpecificCatch", "ArrayEquals"})
    public static boolean fileCopy(
        Object srcFile, Object dstFile,
        boolean move, boolean overwrite, Object filter
    ) {
        final int BUF_SIZE = 1024*8*2;
        File src = ( srcFile instanceof File )
            ? ( (File)srcFile ) : new File( srcFile.toString() )
        ;
        File dst = dstFile!=null ? (
            ( dstFile instanceof File )
            ? ( (File)dstFile ) : new File( dstFile.toString() )
        ) : null;
        if ( filter!=null ) {
            Object[] arr = new Object[3];
            arr[0] = ( move ? ( dst==null?"deleting":"moving" ) : "copying" );
            arr[1]=src; arr[2]=dst;
            if ( !filter.equals( arr ) ) return true;
        }
        boolean dstIsDir = dst==null || dst.isDirectory();
        boolean dstExists = dst!=null && ( dstIsDir || dst.exists() );
        File[] srcDir = src.listFiles();
        if ( srcDir==null && !src.exists() ) return false;
        if ( dst!=null && ( !dstExists ) ) {
            File pf = dst.getParentFile();
            if ( pf!=null ) pf.mkdirs();
        }
        if ( dst!=null && move && src.renameTo( dst ) ) return true;
        if ( dst==null && srcDir==null ) return src.delete();
        if ( srcDir!=null ) {
            Arrays.sort( srcDir );
            boolean success = true;
            for (File newSrc : srcDir) {
                File newDst = dst==null ? null : new File( dst, newSrc.getName() );
                if ( !fileCopy( newSrc, newDst, move, overwrite, filter ) ) success=false;
            }
            if ( move && success ) return src.delete();
            return success;
        } else {
            if ( dstIsDir ) dst=new File( dst, src.getName() );
            FileInputStream fInp = null;
            FileOutputStream fOut = null;
            BufferedInputStream bInp = null;
            BufferedOutputStream bOut = null;
            byte[] buf = new byte[BUF_SIZE];
            try {
                fInp = new FileInputStream( src );
                bInp = new BufferedInputStream( fInp );
                fOut = new FileOutputStream( dst, false );
                bOut = new BufferedOutputStream( fOut );
                while ( true ) {
                    int readCount = bInp.read( buf );
                    if ( readCount>0 ) {
                        bOut.write( buf, 0, readCount );
                    } else
                    if ( readCount<0 ) {
                        break;
                    } else {
                        Thread.sleep( 100 );
                    }
                }
            } catch ( InterruptedException abort ) {
                return false;
            } catch ( IOException ioe ) {
                System.err.println( ioe.getMessage() );
                return false;
            } finally {
                try { if(bOut!=null)bOut.flush(); } catch ( Throwable ignore ) {}
                try { if(fOut!=null)fOut.flush(); } catch ( Throwable ignore ) {}
                try { if(bOut!=null)bOut.close(); } catch ( Throwable ignore ) {}
                try { if(fOut!=null)fOut.close(); } catch ( Throwable ignore ) {}
                try { if(bInp!=null)bInp.close(); } catch ( Throwable ignore ) {}
                try { if(fInp!=null)fInp.close(); } catch ( Throwable ignore ) {}
            }
            dst.setLastModified( src.lastModified() );
            if ( move ) return src.delete();
            return true;
        }
    }
    @SuppressWarnings("unused")
    private static boolean fileCopy_TEST_() {
        if ( System.currentTimeMillis()>0 ) return true;
        @SuppressWarnings("overrides")
        Object filter = new Object(){ public boolean equals( Object o ){
            if ( o instanceof List ) {
                Iterator<?> it = ( (List<?>)o ).iterator();
                int idx = 0;
                while ( it.hasNext() ) {
                    Object x = it.next();
                    if ( x==null ) continue;
                    if ( idx>0 ) System.out.print( ' ' );
                    System.out.print( x );
                    idx++;
                }
                System.out.println();
            } else {
                System.out.println( o );
            }
            return true;
        } };
        {
            fileCopy( "c:/WINDOWS/system32/LogFiles/Fax", "c:/logs.bak", false, true, filter );
            fileCopy( "c:/WINDOWS/system.ini", "c:/logs.bak", false, true, filter );
            fileCopy( "c:\\logs.bak", "c:/logs.COPY", true, true, filter );
            fileCopy( "C:/logs.COPY", "C:/logs.COPY/a/b\\c", true, false, filter );
            fileCopy( "C:/logs.COPY", "C:/logs.bak", true, true, filter );
            fileCopy( "C:/logs.bak", null, true, false, filter );
        }
        if ( !fileCopy(
            "C:/WINDOWS/system32/LogFiles/Fax", "c:/test.tmp/LogFiles",
            false, true, filter
        ) ) return false;
        fileCopy( "c:/WINDOWS/system.ini", "c:/test.tmp/LogFiles", false, true, filter );
        if ( !fileCopy(
            "C:/WINDOWS/system32/LogFiles/Fax", "c:/test.tmp/LogFiles",
            false, false, filter
        ) ) return false;
        if ( !new File( "c:/test.tmp/LogFiles" ).isDirectory() ) return false;
        if ( !fileCopy(
            "c:/test.tmp/LogFiles", "c:/test.tmp/Renamed",
            true, true, filter
        ) ) return false;
        if ( new File( "c:/test.tmp/LogFiles" ).exists() ) return false;
        if ( !new File( "c:/test.tmp/Renamed" ).isDirectory() ) return false;
        if ( fileCopy(
             "c:/test.tmp", "c:/test.tmp/Renamed", true, false, filter
        ) ) return false;
        if ( !new File( "c:/test.tmp/Renamed/Renamed" ).isDirectory() ) return false;
        if ( !fileCopy(
            "c:/test.tmp", null, true, false, filter
        ) ) return false;
        if ( new File( "c:/test.tmp" ).exists() ) return false;
        return true;
    }



    public static boolean cp( File src, File dst ) {
        if ( src==null || dst==null ) return false;
        if ( !src.exists() ) return false;
        fileCopy( src, dst, false, true, null );
        return true;
    }



    public static boolean mv( File src, File dst ) {
        if ( src==null || dst==null ) return false;
        if ( !src.exists() ) return false;
        fileCopy( src, dst, true, true, null );
        return rm( src );
    }



    /**
     * Deletes file or recursively deletes directory. Returns true if all files are gone.
     * If fails, puts out a deleteOnExit hit on every file, then returns false.
    **/
    public static boolean rm( File f ) {
        if ( f==null ) return false;
        if ( f.isDirectory() ) {
            File[] fArr = f.listFiles();
            for ( int i=0; i<fArr.length; i++ ) rm( fArr[i] );
        }
        boolean success = f.delete() && !f.exists();
        if ( !success ) f.deleteOnExit();
        return success;
    }
    public static boolean rm( Path f ) { return f==null?false:rm( f.toFile() ); }
    public static boolean rm( String f ) { return f==null?false:rm( new File( f ) ); }
    @SuppressWarnings("unused")
    private static boolean rm_TEST_( boolean findLineNumber ) throws Exception {
        if ( findLineNumber ) throw new RuntimeException();
        File f = new File( "rm_TEST_"+Lib.timeStamp() );
        f.mkdirs();
        f.createNewFile();
        LibTest.asrt( f.exists() );
        LibTest.asrt( rm( f ) );
        LibTest.asrt( !f.exists() );
        {
            File f2 = new File( "rm_TEST_"+Lib.timeStamp() );
            f2.mkdirs();
            for ( int i=0; i<10; i++ ) {
                File f3 = new File( f2, "rm_TEST_"+Lib.timeStamp() );
                f3.createNewFile();
            }
            LibTest.asrt( rm( f2 ) );
            LibTest.asrt( !f2.exists() );
        }
        return true;
    }



    public static char[] readFully( Reader r ) {
        return LibIO.readFully( r );
    }
    public static byte[] readFully( InputStream inp ) {
        return LibIO.readFully( inp );
    }



    /**
    * Appends to the given file in a reasonably synchronized way. Not fast.
    **/
    public static Throwable append2file( File f, Object data ) {
        return LibIO.append2file( f, data );
    }
    @SuppressWarnings("unused")
    private static boolean append2file_TEST_() throws IOException {
        File f = File.createTempFile( "append2file_TEST_", ".tmp" );
        f.deleteOnExit();
        Throwable t = append2file( f, "Hello, world!" );
        if ( t!=null ) throw new RuntimeException( t );
        t = append2file( f, " Goodbye." );
        if ( t!=null ) throw new RuntimeException( t );
        LibTest.asrtEQ( "Hello, world! Goodbye.", file2string( f, null ) );
        LibTest.asrt( f.delete() );
        return true;
    }



    /**
     * everything after the last dot, if any, inclusive
    **/
    public static String getFileExtension( String fileName ) {
        int dotIdx = fileName.lastIndexOf( "." );
        if ( dotIdx<0 ) return "";
        int sepIdx = -1;
        sepIdx = Math.max( sepIdx, fileName.lastIndexOf( '\\' ) );
        sepIdx = Math.max( sepIdx, fileName.lastIndexOf( '/' ) );
        sepIdx = Math.max( sepIdx, fileName.lastIndexOf( ':' ) );
        if ( dotIdx<sepIdx ) return "";
        return fileName.substring( dotIdx );
    }
    @SuppressWarnings("unused")
    private static boolean getFileExtension_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrtEQ( getFileExtension( "a/b/c.ext" ), ".ext" );
        LibTest.asrtEQ( getFileExtension( "a/b/c" ), "" );
        LibTest.asrtEQ( getFileExtension( "a/b/c.ext/" ), "" );
        LibTest.asrtEQ( getFileExtension( "a/b/c.ext/def" ), "" );
        return true;
    }



    public static String getMimeType( File f ) {
        return getMimeType( f.getName() );
    }
    public static String getMimeType( String uri ) {
        FileNameMap fileNameMap = URLConnection.getFileNameMap();
        String mimeType = fileNameMap.getContentTypeFor( uri );
        if ( mimeType!=null ) return mimeType;
        String ext = getFileExtension( uri );
        if ( ext==null ) ext="";
        ext = ext.toUpperCase();
        switch ( ext ) {
            case ".OUT": return "text/plain";
            case ".ICO": return "image/x-icon";
            default:
                if ( ext.endsWith( "ML" ) ) return "text/xml";
                return "application/octet-stream";
        }
    }
    @SuppressWarnings("unused")
    private static boolean getMimeType_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        LibTest.asrtEQ( "text/plain", getMimeType( "test.txt" ) );
        LibTest.asrtEQ( "image/gif", getMimeType( "test.gif" ) );
        LibTest.asrtEQ( "image/jpeg", getMimeType( "test.jpg" ) );
        LibTest.asrtEQ( "image/jpeg", getMimeType( "test.jpeg" ) );
        LibTest.asrtEQ( "image/png", getMimeType( "test.png" ) );
        return true;
    }



    public static String safeForFilename( String tryBaseName ) {
        return tryBaseName.replaceAll( "[/\\\\:*?\"<>|\\x00-\\x1F\\x7F]+", "_" );
    }



    /**
     * Creates a new full-path filename with a timestamp prefix or suffix.
     * Replaces the timestamp if it already exists in the filename.
     * Can also remove the timestamp via undo
    **/
    public static String backupFilespec( String fspec ) {
        return backupFilespec( fspec, false, true, false, null, null );
    }
    public static String backupFilespec(
        String fspec, boolean undo, boolean prefix, boolean suffix, Long atTime, String swapDir
    ) {
        Path path = Paths.get( fspec );
        String dirname = path.getParent()!=null ? path.getParent().toString() : "";
        String basename = path.getFileName().toString();
        if ( swapDir!=null ) dirname=swapDir;
        if ( atTime==null ) atTime=Lib.currentTimeMicros();
        String stamp = Lib.timeStamp( atTime, true, null ) + "P" + ProcessHandle.current().pid();
        Pattern pattern = Pattern.compile(
            "(_*)\\d{4}\\D\\d{2}\\D\\d{2}\\D\\d{2}\\D\\d{2}\\D\\d{2}\\D\\d{1,9}P\\d+(_*)"
        );
        Matcher matcher = pattern.matcher( basename );
        basename = matcher.replaceAll( "" );
        if ( undo ) return Paths.get( dirname, basename ).toString();
        int dotIndex = basename.lastIndexOf( '.' );
        String name = ( dotIndex==-1 ) ? basename : basename.substring( 0, dotIndex );
        String ext = ( dotIndex==-1 ) ? "" : basename.substring( dotIndex );
        if ( prefix && !suffix ) {
            basename = stamp + "_" + name + ext;
        } else {
            basename = name + "_" + stamp + ext;
        }
        return Paths.get( dirname, basename ).toString();
    }
    @SuppressWarnings("unused")
    private static boolean backupFilespec_TEST_( boolean findLineNumber ) {
        if ( findLineNumber ) throw new RuntimeException();
        String orig = "/not/real/dir/test.txt";
        String backupFspec = backupFilespec( orig );
        LibTest.asrt( backupFspec.length()>orig.length(), "Backup filename should be longer than original" );
        String actual = backupFilespec( backupFspec, true, true, false, null, null );
        actual = actual.replace( '\\', '/' );
        String expected = orig;
        LibTest.asrtEQ( actual, expected, "Undo should restore original filename" );
        actual = backupFilespec( backupFspec, false, true, false, null,  "/new/dir" );
        actual = actual.replace( '\\', '/' );
        expected = "^/new/dir/.*";
        LibTest.asrt( actual.matches( expected ), "Directory swap failed" );
        return true;
    }



    public static File unzip( File srcZip, File destDir, String zipSubDir, Object filter ) throws IOException {
        boolean bubbleUpException = filter==null;
        ZipFile zf = new ZipFile( srcZip );
        zipSubDir = normalizeEntryName( zipSubDir );
        try {
            if ( destDir==null ) {
                destDir = createUniqFile( new File(
                    System.getProperty( "java.io.tmpdir" ), new File( zf.getName() ).getName()
                ), true );
            }
            @SuppressWarnings("rawtypes")
            Enumeration entryEnum = zf.entries();
            while( entryEnum.hasMoreElements() ) {
                ZipEntry entry = (ZipEntry)entryEnum.nextElement();
                String entryName = normalizeEntryName( entry.getName() );
                if ( !entryName.startsWith( zipSubDir ) ) continue;
                entryName = normalizeEntryName( entryName.substring( zipSubDir.length() ) );
                File dstFile = new File( destDir, entryName );
                Object[] msg = new Object[]{ "processing", entry, entryName, dstFile, zf };
                if ( filter!=null && !filter.equals( msg ) ) continue;
                boolean srcIsDir = entry.isDirectory();
                boolean dstIsDir = dstFile.isDirectory() || ( ( !dstFile.exists() ) && srcIsDir );
                if ( srcIsDir && ( !dstIsDir ) ) {
                    dstFile.delete();
                    dstIsDir = true;
                }
                if ( dstIsDir && ( !srcIsDir ) ) {
                    rm( dstFile );
                    dstIsDir = false;
                }
                if ( dstIsDir ) {
                    if ( !dstFile.exists() ) {
                        if ( !dstFile.mkdirs() )
                            throw new IOException( "couldn't create dir: "+getCanonicalPath( dstFile ) );
                    }
                    continue;
                }
                File parent = dstFile.getParentFile();
                if ( !parent.isDirectory() && !parent.mkdirs() )
                    throw new IOException( "couldn't create parent dir for: "+dstFile );
                InputStream in = zf.getInputStream( entry );
                ByteArrayOutputStream baoStream = new ByteArrayOutputStream();
                byte[] buffer = new byte[10240];
                int bytesRead;
                while ( ( bytesRead = in.read( buffer ) )!=-1 ) {
                    baoStream.write( buffer, 0, bytesRead );
                }
                in.close();
                byte[] fromZip = baoStream.toByteArray();
                boolean needWrite = true;
                if ( dstFile.exists() && dstFile.length()==fromZip.length ) {
                    byte[] fromFile = readBinaryFile( dstFile );
                    needWrite = !Arrays.equals( fromZip, fromFile );
                }
                if ( !needWrite ) continue;
                for ( int retry=0; retry<2; retry++ ) {
                    try {
                        FileOutputStream fos = new FileOutputStream( dstFile );
                        fos.write( fromZip );
                        fos.close();
                        break;
                    } catch ( IOException e ) {
                        if ( retry>0 ) {
                            if ( filter!=null ) {
                                msg[0] = "failed";
                                filter.equals( msg );
                            }
                            if ( bubbleUpException ) throw new IOException( "couldn't write: "+getCanonicalPath( dstFile ) );
                        }
                        try { Thread.sleep( 100 ); } catch ( InterruptedException ie ) {}
                    }
                }
            }
        } finally {
            try { zf.close(); }catch( Throwable ignore ){}
        }
        return destDir;
    }
    private static String normalizeEntryName( String nam ) {
        if ( nam==null ) nam = "";
        if ( nam.startsWith( "/" ) || nam.startsWith( "\\" ) ) nam = nam.substring( 1 );
        nam = nam.replace( '\\', '/' );
        return nam;
    }
    private static File createUniqFile( File proto, boolean isDir ) {
        if ( !proto.exists() ) return proto;
        String base = proto.getName();
        String ext = "";
        int dotIdx = base.lastIndexOf( '.' );
        if ( dotIdx>=0 ) {
            ext = base.substring( dotIdx );
            base = base.substring( 0, dotIdx );
        }
        File parent = proto.getParentFile();
        for ( int i=1; i<Integer.MAX_VALUE; i++ ) {
            File f = new File( parent, base+"_"+i+ext );
            if ( !f.exists() ) return f;
        }
        throw new RuntimeException( "Can't create unique file" );
    }
    private static byte[] readBinaryFile( File f ) throws IOException {
        FileInputStream fis = new FileInputStream( f );
        ByteArrayOutputStream baos = new ByteArrayOutputStream();
        byte[] buffer = new byte[10240];
        int bytesRead;
        while ( ( bytesRead = fis.read( buffer ) )!=-1 ) {
            baos.write( buffer, 0, bytesRead );
        }
        fis.close();
        return baos.toByteArray();
    }






    public static String getCanonicalPath( File f ) {
        try { return f.getCanonicalPath(); }
        catch ( IOException e ) { return f.getAbsolutePath(); }
    }



    public static void main( String[] args ) throws Exception {
        LibTest.testClass( LibFile.class );
    }
}
