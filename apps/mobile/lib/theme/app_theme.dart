import 'package:flutter/material.dart';

class AppTheme {
  static const bg = Color(0xFF121212);
  static const sidebar = Color(0xFF1E1E1E);
  static const bubbleIn = Color(0xFF2A2A2A);
  static const accent = Color(0xFFE8893A);
  static const bubbleOut = Color(0xFFE8893A);
  static const textPrimary = Color(0xFFE8E8E8);
  static const textSecondary = Color(0xFF9AA0A6);
  static const border = Color(0xFF2D2D2D);

  static ThemeData dark() {
    return ThemeData(
      brightness: Brightness.dark,
      scaffoldBackgroundColor: bg,
      colorScheme: const ColorScheme.dark(
        primary: accent,
        surface: sidebar,
      ),
      appBarTheme: const AppBarTheme(
        backgroundColor: sidebar,
        foregroundColor: textPrimary,
        elevation: 0,
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: sidebar,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: const BorderSide(color: border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: const BorderSide(color: border),
        ),
      ),
      useMaterial3: true,
    );
  }
}
