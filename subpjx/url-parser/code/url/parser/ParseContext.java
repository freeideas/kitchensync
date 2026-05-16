package url.parser;

public record ParseContext(String current_working_directory, String current_os_user) {
    public ParseContext {
        if (current_working_directory == null) {
            current_working_directory = "";
        }
        if (current_os_user == null) {
            current_os_user = "";
        }
    }

    public String currentWorkingDirectory() {
        return current_working_directory;
    }

    public String currentOsUser() {
        return current_os_user;
    }
}
