// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'sync.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$SyncEvent {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SyncEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'SyncEvent()';
}


}

/// @nodoc
class $SyncEventCopyWith<$Res>  {
$SyncEventCopyWith(SyncEvent _, $Res Function(SyncEvent) __);
}


/// Adds pattern-matching-related methods to [SyncEvent].
extension SyncEventPatterns on SyncEvent {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( SyncEvent_Synced value)?  synced,TResult Function( SyncEvent_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case SyncEvent_Synced() when synced != null:
return synced(_that);case SyncEvent_Failed() when failed != null:
return failed(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( SyncEvent_Synced value)  synced,required TResult Function( SyncEvent_Failed value)  failed,}){
final _that = this;
switch (_that) {
case SyncEvent_Synced():
return synced(_that);case SyncEvent_Failed():
return failed(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( SyncEvent_Synced value)?  synced,TResult? Function( SyncEvent_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case SyncEvent_Synced() when synced != null:
return synced(_that);case SyncEvent_Failed() when failed != null:
return failed(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( SyncSummary summary)?  synced,TResult Function( String message,  bool expired)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case SyncEvent_Synced() when synced != null:
return synced(_that.summary);case SyncEvent_Failed() when failed != null:
return failed(_that.message,_that.expired);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( SyncSummary summary)  synced,required TResult Function( String message,  bool expired)  failed,}) {final _that = this;
switch (_that) {
case SyncEvent_Synced():
return synced(_that.summary);case SyncEvent_Failed():
return failed(_that.message,_that.expired);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( SyncSummary summary)?  synced,TResult? Function( String message,  bool expired)?  failed,}) {final _that = this;
switch (_that) {
case SyncEvent_Synced() when synced != null:
return synced(_that.summary);case SyncEvent_Failed() when failed != null:
return failed(_that.message,_that.expired);case _:
  return null;

}
}

}

/// @nodoc


class SyncEvent_Synced extends SyncEvent {
  const SyncEvent_Synced({required this.summary}): super._();
  

 final  SyncSummary summary;

/// Create a copy of SyncEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SyncEvent_SyncedCopyWith<SyncEvent_Synced> get copyWith => _$SyncEvent_SyncedCopyWithImpl<SyncEvent_Synced>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SyncEvent_Synced&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode {
    return Object.hash(runtimeType,summary);
}

@override
String toString() {
    return 'SyncEvent.synced(summary: $summary)';
}


}

/// @nodoc
abstract mixin class $SyncEvent_SyncedCopyWith<$Res> implements $SyncEventCopyWith<$Res> {
  factory $SyncEvent_SyncedCopyWith(SyncEvent_Synced value, $Res Function(SyncEvent_Synced) _then) = _$SyncEvent_SyncedCopyWithImpl;
@useResult
$Res call({
 SyncSummary summary
});




}
/// @nodoc
class _$SyncEvent_SyncedCopyWithImpl<$Res>
    implements $SyncEvent_SyncedCopyWith<$Res> {
  _$SyncEvent_SyncedCopyWithImpl(this._self, this._then);

  final SyncEvent_Synced _self;
  final $Res Function(SyncEvent_Synced) _then;

/// Create a copy of SyncEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,}) {
  return _then(SyncEvent_Synced(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as SyncSummary,
  ));
}


}

/// @nodoc


class SyncEvent_Failed extends SyncEvent {
  const SyncEvent_Failed({required this.message, required this.expired}): super._();
  

 final  String message;
 final  bool expired;

/// Create a copy of SyncEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SyncEvent_FailedCopyWith<SyncEvent_Failed> get copyWith => _$SyncEvent_FailedCopyWithImpl<SyncEvent_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SyncEvent_Failed&&(identical(other.message, message) || other.message == message)&&(identical(other.expired, expired) || other.expired == expired));
}


@override
int get hashCode {
    return Object.hash(runtimeType,message,expired);
}

@override
String toString() {
    return 'SyncEvent.failed(message: $message, expired: $expired)';
}


}

/// @nodoc
abstract mixin class $SyncEvent_FailedCopyWith<$Res> implements $SyncEventCopyWith<$Res> {
  factory $SyncEvent_FailedCopyWith(SyncEvent_Failed value, $Res Function(SyncEvent_Failed) _then) = _$SyncEvent_FailedCopyWithImpl;
@useResult
$Res call({
 String message, bool expired
});




}
/// @nodoc
class _$SyncEvent_FailedCopyWithImpl<$Res>
    implements $SyncEvent_FailedCopyWith<$Res> {
  _$SyncEvent_FailedCopyWithImpl(this._self, this._then);

  final SyncEvent_Failed _self;
  final $Res Function(SyncEvent_Failed) _then;

/// Create a copy of SyncEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,Object? expired = null,}) {
  return _then(SyncEvent_Failed(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,expired: null == expired ? _self.expired : expired // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

// dart format on
