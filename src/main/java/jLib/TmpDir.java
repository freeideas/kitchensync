package jLib;
import java.io.*;


/**
 * A temporary directory that is automatically cleaned up when closed or garbage collected.
 * Implements AutoCloseable for use in try-with-resources blocks.
 */
public class TmpDir implements AutoCloseable {
    
    public final File dir;
    
    
    
    /**
     * Creates a new temporary directory with a unique name
     */
    @SuppressWarnings("this-escape")
    public TmpDir() {
        dir = new File( System.getProperty( "java.io.tmpdir" ), "tmp_"+Lib.uniqID() );
        dir.mkdirs();
        Lib.cleaner.register( this, ()->LibFile.rm( dir ) );
    }
    
    
    
    /**
     * Deletes the temporary directory and all its contents
     */
    @Override
    public void close() { 
        LibFile.rm( dir ); 
    }
    
    
    
    /**
     * Gets the temporary directory File object
     * @return The temporary directory
     */
    public File getDir() {
        return dir;
    }
    
    
    
    /**
     * Creates a new file in the temporary directory
     * @param name The name of the file to create
     * @return The created file
     */
    public File newFile( String name ) {
        return new File( dir, name );
    }
    
    
    
    /**
     * Creates a new subdirectory in the temporary directory
     * @param name The name of the subdirectory to create
     * @return The created directory
     */
    public File newDir( String name ) {
        File subDir = new File( dir, name );
        subDir.mkdirs();
        return subDir;
    }
    
    
    
    @SuppressWarnings("unused")
    private static boolean _TEST_( boolean findLineNumber ) {
        if (findLineNumber) throw new RuntimeException();
        
        // Test basic creation and cleanup
        File tempFile = null;
        try ( TmpDir tmpDir = new TmpDir() ) {
            LibTest.asrt( tmpDir.dir.exists() );
            LibTest.asrt( tmpDir.dir.isDirectory() );
            
            // Test creating a file
            tempFile = tmpDir.newFile("test.txt");
            LibFile.string2file("test content", tempFile, false);
            LibTest.asrt( tempFile.exists() );
            
            // Test creating a subdirectory
            File subDir = tmpDir.newDir("subdir");
            LibTest.asrt( subDir.exists() );
            LibTest.asrt( subDir.isDirectory() );
            
            // Test file in subdirectory
            File subFile = new File(subDir, "subfile.txt");
            LibFile.string2file("sub content", subFile, false);
            LibTest.asrt( subFile.exists() );
        }
        
        // Verify cleanup after close
        LibTest.asrt( !tempFile.exists() );
        
        return true;
    }
    
    
    
    public static void main( String[] args ) throws Exception {
        LibTest.testClass( TmpDir.class );
    }
}