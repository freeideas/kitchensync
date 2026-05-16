package url.parser;

public enum ParseErrorCategory {
    empty_operand,
    invalid_role_prefix,
    invalid_fallback_group,
    unsupported_scheme,
    invalid_file_url,
    invalid_sftp_url,
    invalid_setting,
    invalid_percent_encoding,
    invalid_context
}
