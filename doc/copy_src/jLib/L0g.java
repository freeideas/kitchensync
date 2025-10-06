package jLib;
import java.sql.*;
import java.util.concurrent.atomic.*;
import java.util.concurrent.*;
import java.io.*;


/**
 * Dual-mode logging utilities following LOGGING.md specification.
 * 
 * Automatically detects SQLite JDBC driver availability:
 * - If SQLite driver is present: logs to database (l0g.db)
 * - If SQLite driver is missing: logs to console (System.out)
 * 
 * This allows projects to choose logging method by including/excluding
 * the SQLite dependency, without any code changes.
 */
public class L0g {
    
    private static final AtomicReference<String> appId = new AtomicReference<>();
    private static final AtomicReference<String> dbPath = new AtomicReference<>("l0g.db");
    private static final LruCache<String,Long> logOnceCache = new LruCache<>( -1, 1000*60*60, false );
    private static final ConcurrentHashMap<String,Connection> connections = new ConcurrentHashMap<>();
    private static final boolean useDatabaseLogging;
    
    // Initialize database on first use
    static {
        boolean dbAvailable = false;
        try {
            Class.forName("org.sqlite.JDBC");
            dbAvailable = true;
        } catch (ClassNotFoundException e) {
            // SQLite not available, will use console logging
        }
        useDatabaseLogging = dbAvailable;
    }
    
    
    
    public static void log(String descr) {
        log(descr, false, null, null);
    }
    
    
    
    public static void log(Throwable throwable) {
        log(throwable, null, null);
    }
    
    
    
    public static void log(String descr, Boolean isError, String eventType, String sessionId) {
        if (descr == null) throw new IllegalArgumentException("descr cannot be null");
        if (isError == null) isError = false;
        if (eventType == null) eventType = "info";
        logInternal(isError, eventType, descr, sessionId);
    }
    
    
    
    public static void log(Throwable throwable, String eventType, String sessionId) {
        if (throwable == null) throw new IllegalArgumentException("throwable cannot be null");
        if (eventType == null) eventType = "error";
        String descr = throwable.getClass().getSimpleName() + ": " + throwable.getMessage();
        logInternalWithThrowable(true, eventType, descr, sessionId, throwable);
    }
    
    
    
    /**
     * Get stack trace of caller
     */
    private static String getStackTrace() {
        StackTraceElement[] stackTrace = Thread.currentThread().getStackTrace();
        // Skip getStackTrace(), log(), and public logging methods
        for (int i = 3; i < stackTrace.length; i++) {
            StackTraceElement elem = stackTrace[i];
            String className = elem.getClassName();
            // Skip our own logging methods
            if (!className.equals("jLib.L0g") && !className.startsWith("java.")) {
                StringBuilder sb = new StringBuilder();
                sb.append("at ").append(elem.getClassName()).append(".").append(elem.getMethodName());
                sb.append("(").append(elem.getFileName()).append(":").append(elem.getLineNumber()).append(")");
                return sb.toString();
            }
        }
        return "unknown";
    }
    
    
    
    /**
     * Core logging method that writes to database
     */
    private static void logInternal(boolean isError, String eventType, String descr, String sessionId) {
        logInternalWithThrowable(isError, eventType, descr, sessionId, null);
    }
    
    
    
    /**
     * Core logging method that writes to database with optional throwable
     */
    private static void logInternalWithThrowable(boolean isError, String eventType, String descr, String sessionId, Throwable throwable) {
        if (useDatabaseLogging) {
            // Use database logging
            try {
                Connection conn = getConnection();
                ensureTableExists(conn);
                
                String stackTrace = throwable != null ? getThrowableStackTrace(throwable) : getStackTrace();
                String sql = "INSERT INTO l0g (app_id, event_type, descr, is_error, stack_trace" +
                            (sessionId != null ? ", session_id" : "") + 
                            ") VALUES (?, ?, ?, ?, ?" +
                            (sessionId != null ? ", ?" : "") + ")";
                
                try (PreparedStatement pstmt = conn.prepareStatement(sql)) {
                    pstmt.setString(1, getAppId());
                    pstmt.setString(2, eventType);
                    pstmt.setString(3, descr);
                    pstmt.setBoolean(4, isError);
                    pstmt.setString(5, stackTrace);
                    if (sessionId != null) {
                        pstmt.setString(6, sessionId);
                    }
                    pstmt.executeUpdate();
                }
            } catch (SQLException e) {
                // Fallback to console if database logging fails
                logToConsole(isError, eventType, descr, sessionId, throwable);
            }
        } else {
            // Use console logging
            logToConsole(isError, eventType, descr, sessionId, throwable);
        }
    }
    
    
    
    /**
     * Get stack trace from throwable
     */
    private static String getThrowableStackTrace(Throwable throwable) {
        StringWriter sw = new StringWriter();
        PrintWriter pw = new PrintWriter(sw);
        throwable.printStackTrace(pw);
        return sw.toString();
    }
    
    
    
    /**
     * Log to console when database is not available
     */
    private static void logToConsole(boolean isError, String eventType, String descr, String sessionId, Throwable throwable) {
        // Format: [timestamp] [app_id] [ERROR/INFO] [event_type] descr [session_id] [stack_trace]
        StringBuilder sb = new StringBuilder();
        
        // Timestamp
        sb.append("[").append(new java.text.SimpleDateFormat("yyyy-MM-dd HH:mm:ss.SSS").format(new java.util.Date())).append("] ");
        
        // App ID
        sb.append("[").append(getAppId()).append("] ");
        
        // Error level
        sb.append("[").append(isError ? "ERROR" : "INFO").append("] ");
        
        // Event type
        sb.append("[").append(eventType).append("] ");
        
        // Description
        sb.append(descr);
        
        // Session ID if present
        if (sessionId != null) {
            sb.append(" [session:").append(sessionId).append("]");
        }
        
        // Stack trace
        String stackTrace = throwable != null ? getThrowableStackTrace(throwable) : getStackTrace();
        sb.append(" ").append(stackTrace);
        
        // Output to System.out
        System.out.println(sb.toString());
        
        // If throwable provided, also print full stack trace
        if (throwable != null && isError) {
            throwable.printStackTrace(System.out);
        }
    }
    
    
    
    /**
     * Log once functionality - converted to database logging
     */
    public static boolean logOnce(String eventType, String descr) { 
        return logOnce(eventType, descr, 5000); 
    }
    
    
    
    public static boolean logOnce(String eventType, String descr, long perMillis) { 
        return logOnce(null, eventType, descr, perMillis); 
    }
    
    
    
    public static boolean logOnce(String msgID, String eventType, String descr, long perMillis) {
        if (perMillis < 1) perMillis = 5000;
        if (msgID == null) msgID = eventType + ":" + descr;
        
        boolean didLog = false;
        long now = System.currentTimeMillis();
        Long lastLogTime = logOnceCache.get(msgID);
        if (lastLogTime == null) {
            lastLogTime = 0L;
        }
        if (now - lastLogTime > perMillis) {
            didLog = true;
            log(descr, false, eventType, null);
            logOnceCache.put(msgID, now);
        }
        return didLog;
    }
    
    
    
    /**
     * Get database connection, creating if necessary
     */
    private static Connection getConnection() throws SQLException {
        String path = dbPath.get();
        Connection conn = connections.get(path);
        
        if (conn == null || conn.isClosed()) {
            // Ensure directory exists
            File dbFile = new File(path);
            File parentDir = dbFile.getParentFile();
            if (parentDir != null && !parentDir.exists()) {
                parentDir.mkdirs();
            }
            
            conn = DriverManager.getConnection("jdbc:sqlite:" + path);
            connections.put(path, conn);
        }
        return conn;
    }
    
    
    
    /**
     * Ensure the l0g table exists
     */
    private static void ensureTableExists(Connection conn) throws SQLException {
        String createTable = 
            "CREATE TABLE IF NOT EXISTS l0g (" +
            "id INTEGER PRIMARY KEY AUTOINCREMENT, " +
            "stamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f','now','utc')), " +
            "app_id TEXT NOT NULL, " +
            "event_type TEXT NOT NULL, " +
            "is_error BOOLEAN NOT NULL DEFAULT 0, " +
            "stack_trace TEXT, " +
            "session_id TEXT, " +
            "descr TEXT NOT NULL)";
        
        String[] indexes = {
            "CREATE INDEX IF NOT EXISTS idx_event_type ON l0g(event_type)",
            "CREATE INDEX IF NOT EXISTS idx_session_id ON l0g(session_id)",
            "CREATE INDEX IF NOT EXISTS idx_app_id ON l0g(app_id)",
            "CREATE INDEX IF NOT EXISTS idx_is_error ON l0g(is_error)"
        };
        
        try (Statement stmt = conn.createStatement()) {
            stmt.execute(createTable);
            for (String index : indexes) {
                stmt.execute(index);
            }
        }
    }
    
    
    
    /**
     * Get the application ID
     */
    public static String getAppId() {
        String id = appId.get();
        if (id == null) {
            // Auto-detect from main class or use default
            id = LibApp.getAppName();
            appId.set(id);
        }
        return id;
    }
    
    
    
    /**
     * Set custom application ID
     */
    public static void SetAppId(String id) {
        appId.set(id);
    }
    
    
    
    /**
     * Set custom database path
     */
    public static void SetDbPath(String path) {
        dbPath.set(path);
    }
    
    
    
    /**
     * Check if database logging is enabled
     * @return true if SQLite driver is available and database logging is being used
     */
    public static boolean isDatabaseLoggingEnabled() {
        return useDatabaseLogging;
    }
    
    
    
    // ========== Compatibility methods for old Log.java API ==========
    
    /**
     * Log an object - compatibility method for old Log.java
     * @param o Object to log (can be null, Throwable, or any object)
     * @return The same object that was passed in
     */
    public static Object log(Object o) {
        if (o == null) {
            log("null");
        } else if (o instanceof Throwable) {
            log((Throwable) o);
        } else {
            String msg = o.toString();
            try {
                // Try to use JsonEncoder if available
                msg = JsonEncoder.encode(o);
            } catch (Throwable ignore) {
                // Fall back to toString if JsonEncoder not available
            }
            log(msg);
        }
        return o;
    }
    
    
    
    /**
     * Log once with just object - compatibility method for old Log.java
     * @param o Object to log
     * @return true if the message was logged, false if it was suppressed
     */
    public static boolean logOnce(Object o) {
        return logOnce(o, 5000);
    }
    
    
    
    /**
     * Log once with object and time - compatibility method for old Log.java
     * @param o Object to log
     * @param perMillis Minimum milliseconds between logging the same message
     * @return true if the message was logged, false if it was suppressed
     */
    public static boolean logOnce(Object o, long perMillis) {
        return logOnce(null, o, perMillis);
    }
    
    
    
    /**
     * Log once with msgID and object - compatibility method for old Log.java
     * @param msgID Unique identifier for this message (can be null)
     * @param msg The message to log
     * @param perMillis Minimum milliseconds between logging the same message
     * @return true if the message was logged, false if it was suppressed
     */
    public static boolean logOnce(String msgID, Object msg, long perMillis) {
        if (msg == null) msg = msgID;
        String msgTxt = msg.toString();
        if (msgID == null) msgID = msgTxt;
        
        // Convert to new L0g API
        return logOnce(msgID, "info", msgTxt, perMillis);
    }
    
    
    
    @SuppressWarnings("unused")
    private static boolean logOnce_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        // Clear the cache before testing
        logOnceCache.clear();
        
        // Test basic logOnce - use a longer interval to account for processing time
        boolean first = logOnce("test_event", "testing logOnce", 2000);
        boolean second = logOnce("test_event", "testing logOnce", 2000);
        LibTest.asrt(first);
        LibTest.asrt(!second);
        try{ Thread.sleep(2500); }catch(InterruptedException ignore){}
        LibTest.asrt( logOnce("test_event", "testing logOnce", 2000) );
        
        // Test with msgID
        LibTest.asrt( logOnce("msgID1", "test_event", "message 1", 100) );
        LibTest.asrt(!logOnce("msgID1", "test_event", "message 1", 100) );
        LibTest.asrt( logOnce("msgID2", "test_event", "message 2", 100) );
        
        return true;
    }
    
    
    
    @SuppressWarnings("unused")
    private static boolean log_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        // Test basic logging
        log("Test info message");
        log("Test info message with event", false, "test", null);
        log("Test error message", true, "test", null);
        
        // Test throwable logging
        try {
            throw new RuntimeException("Test exception");
        } catch (Exception e) {
            log(e);
            log(e, "exception_test", null);
        }
        
        // Test with session ID
        String sessionId = "test_session_" + System.currentTimeMillis();
        log("Test request", false, "request", sessionId);
        log("Test response", false, "response", sessionId);
        
        // Verify database was created only if database logging is enabled
        if (useDatabaseLogging) {
            File dbFile = new File(dbPath.get());
            LibTest.asrt(dbFile.exists());
        }
        
        return true;
    }
    
    
    
    @SuppressWarnings("unused")
    private static boolean dbConnection_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        // Only test database functionality if database logging is enabled
        if (useDatabaseLogging) {
            // Test custom DB path
            String testDbPath = "./tmp/test_l0g.db";
            SetDbPath(testDbPath);
            
            // Log something to create DB
            log("Testing custom DB path");
            
            // Verify custom DB was created
            File customDb = new File(testDbPath);
            LibTest.asrt(customDb.exists());
            
            // Reset to default
            SetDbPath("l0g.db");
        } else {
            // Just test that console logging works
            log("Testing console logging mode");
        }
        
        return true;
    }
    
    
    
    public static void main(String[] args) throws Exception {
        LibTest.testClass(L0g.class);
    }
}